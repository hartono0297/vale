//! Layout math for the island, in logical pixels (96 DPI). Pure functions, so
//! the renderer only draws what this module decides.

use super::dock::DockZone;
use super::icons::Icon;
use super::panel_layout::Region;
use crate::config::Style;
use crate::i18n::Key;
use crate::model::{Hit, Notice, NoticeKind, TranscriptOffer};
use crate::pomodoro::{Phase, PomoStatus};
use crate::theme::{Palette, radius};

pub const WINDOW_W: f32 = 620.0;
pub const WINDOW_H: f32 = 330.0;
/// The window's height while the enlarged CLI panel shows or morphs: the
/// Pill's top gap, 440 px and the morph's overshoot.
pub const TALL_WINDOW_H: f32 = 460.0;
pub const ANIM_MS: u64 = 240;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeKind {
    Idle,
    MediaCompact,
    /// The compact media view showing a lyric line: wider, same height.
    MediaLyric,
    PomodoroCompact,
    /// A recording runs: the red dot, the clock and the level bars.
    RecordCompact,
    /// The closed island shrunk to the mascot's face while the pointer is
    /// near it over a window (`pointer_watch::Peek`): `EYES_W` x `EYES_H`,
    /// centred, at the compact shapes' top.
    Eyes,
    Notice,
    /// The screenshot preview (keputusan-v3.1 p6): 10 px padding, a
    /// 230 × 136 thumbnail and a right column of 178 px, radius 26.
    NoticeShot,
    Panel,
    /// The CLI page enlarged: the panel 440 px tall.
    PanelTall,
    /// The cleaner over the System page: seven rows and a footer, 352 px
    /// tall (`cleanPanel2` in deepclean-opsi-d.html).
    PanelClean,
}

/// The open panel's shape: the CLI page enlarged when asked for, the
/// cleaner taller than a page, every other page the same.
pub fn panel_kind(page: crate::model::Page, cli_tall: bool, cleaner: bool) -> ShapeKind {
    use crate::model::Page;
    match page {
        Page::Cli if cli_tall => ShapeKind::PanelTall,
        Page::System if cleaner => ShapeKind::PanelClean,
        _ => ShapeKind::Panel,
    }
}

/// Pill floats this far below the top edge; Notch is flush with it.
const PILL_TOP: f32 = 8.0;
/// Bottom corners of the Notch's compact shapes.
const NOTCH_COMPACT_RADIUS: f32 = 16.0;
/// The inset of the compact views' content (the media art or the robot, the
/// equalizer bars, the pomodoro ring, the robot beside a lyric) from the
/// shape's left and right edges. Equal to the Notch's compact corner radius,
/// and the same in the Pill style, so content never sits closer to an end
/// than the curve reaches. Idle, notices, panels and the eyes do not use it.
pub const COMPACT_INSET: f32 = NOTCH_COMPACT_RADIUS;
/// How much wider MediaCompact is than the user's width: the art's and the
/// bars' insets grew from 6 and 14 to `COMPACT_INSET` (+10 and +2), and the
/// shape grows by their sum so the title keeps exactly the room it had.
pub const MEDIA_COMPACT_EXTRA: f32 = 12.0;
/// The same for PomodoroCompact: the ring's inset grew from 14 to 16 (the
/// ring's left edge, +2) on top of the 20 it was already wider.
pub const POMODORO_COMPACT_EXTRA: f32 = 22.0;
/// RecordCompact is this much wider than the user's width: room for the dot,
/// "h:mm:ss" and the two level meters with air between them.
pub const RECORD_COMPACT_EXTRA: f32 = 24.0;
/// The REC view, and the caption view that takes its place when no
/// recording runs, is this much wider while a caption line shows: room for
/// about twenty characters in 11 pt after the clock.
pub const REC_CAPTION_EXTRA: f32 = 120.0;

/// The closed island's width range (Settings -> Tampilan -> Notch), logical
/// px. 160 keeps the idle robot and clock, and the compact media title, inside
/// the shape; 300 is the widest that still reads as a notch.
pub const NOTCH_W_MIN: u32 = 160;
pub const NOTCH_W_MAX: u32 = 300;
pub const NOTCH_W_DEFAULT: u32 = 190;
/// The closed island's height range. 28 is Bata's 26 px plus 1 px each side;
/// 44 stays well under the notice's 76, so a notice still grows from it. The
/// Notch's corner radius (16) fits at 28 (`corner_radius` keeps it at
/// `h - SHOULDER`).
pub const NOTCH_H_MIN: u32 = 28;
pub const NOTCH_H_MAX: u32 = 44;
pub const NOTCH_H_DEFAULT: u32 = 34;
/// The see-through fill's default opacity, percent.
pub const NOTCH_OPACITY_DEFAULT: u8 = 30;

/// The closed island's size as the user set it: what Idle, MediaCompact,
/// PomodoroCompact (`POMODORO_COMPACT_EXTRA` wider), MediaCompact
/// (`MEDIA_COMPACT_EXTRA` wider) and MediaLyric (its minimum width) are built
/// on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotchSize {
    pub w: f32,
    pub h: f32,
}

impl NotchSize {
    /// 190 x 34, the island as it always was.
    pub const DEFAULT: NotchSize = NotchSize {
        w: NOTCH_W_DEFAULT as f32,
        h: NOTCH_H_DEFAULT as f32,
    };

    /// From whole px, clamped to the allowed range.
    pub fn from_px(w: u32, h: u32) -> NotchSize {
        NotchSize {
            w: w.clamp(NOTCH_W_MIN, NOTCH_W_MAX) as f32,
            h: h.clamp(NOTCH_H_MIN, NOTCH_H_MAX) as f32,
        }
    }

    pub fn from_config(config: &crate::config::Config) -> NotchSize {
        NotchSize::from_px(config.notch_width, config.notch_height)
    }
}

pub fn shape_for(style: Style, kind: ShapeKind) -> ShapeRect {
    shape_sized(style, NotchSize::DEFAULT, kind)
}

/// `shape_for` with the closed island at the user's size. The panel and the
/// notices do not follow it.
pub fn shape_sized(style: Style, size: NotchSize, kind: ShapeKind) -> ShapeRect {
    let (w, h) = match kind {
        ShapeKind::Idle => (size.w, size.h),
        ShapeKind::MediaCompact => (size.w + MEDIA_COMPACT_EXTRA, size.h),
        ShapeKind::PomodoroCompact => (size.w + POMODORO_COMPACT_EXTRA, size.h),
        ShapeKind::RecordCompact => (size.w + RECORD_COMPACT_EXTRA, size.h),
        ShapeKind::Eyes => (EYES_W, EYES_H),
        // Sized by its line (`lyric_shape_sized`); without one, the narrowest.
        ShapeKind::MediaLyric => (LYRIC_MIN_W.max(size.w), size.h),
        ShapeKind::Notice => (360.0, 76.0),
        ShapeKind::NoticeShot => (SHOT_W, SHOT_H),
        ShapeKind::Panel => (560.0, 290.0),
        ShapeKind::PanelTall => (560.0, 440.0),
        ShapeKind::PanelClean => (560.0, 352.0),
    };
    // The panel and notices round the same in both styles; compact shapes
    // are a full pill floating, a gentler curve hanging from the edge.
    let radius = match kind {
        ShapeKind::Panel | ShapeKind::PanelTall | ShapeKind::PanelClean => radius::PANEL,
        ShapeKind::Notice | ShapeKind::NoticeShot => radius::NOTICE,
        // Fully rounded; in the Notch style its shoulders are half its
        // height (`shoulder_depth`), so the bottom corners keep this too.
        ShapeKind::Eyes => EYES_H / 2.0,
        _ if style == Style::Pill => h / 2.0,
        _ => NOTCH_COMPACT_RADIUS,
    };
    let y = match style {
        Style::Pill => PILL_TOP,
        Style::Notch => 0.0,
    };
    ShapeRect {
        x: (WINDOW_W - w) / 2.0,
        y,
        w,
        h,
        radius,
    }
}

/// The shrunk notch (`ShapeKind::Eyes`): just room for the mascot's face.
pub const EYES_W: f32 = 34.0;
pub const EYES_H: f32 = 14.0;

/// Bata in a compact island: this big, this far in from the left edge, and
/// this far from what follows. On a notice it takes the badge's 40 px place.
pub const ROBOT_COMPACT: f32 = 26.0;
pub const ROBOT_NOTICE: f32 = 40.0;
const ROBOT_GAP: f32 = 8.0;
/// The idle clock's box beside the robot: "00:00" in DM Sans 600 16,
/// tracked (40.3 px), fits.
pub const CLOCK_W: f32 = 44.0;

/// How much wider a compact shape is with the robot at its left, so its
/// content keeps the room it has without it. Idle holds the robot and the
/// clock as it is; a notice's robot takes the badge's place.
pub fn robot_extra(kind: ShapeKind) -> f32 {
    match kind {
        // The art (22 px, text from +48) becomes the robot (26 px, text from +50).
        ShapeKind::MediaCompact => 2.0,
        // `lyric_width_min` already counts the robot in its lead.
        // The ring's centre was 24 px in; now after the robot, 58 px in.
        ShapeKind::PomodoroCompact => 34.0,
        // The robot (26 px) and its 8 px gap, as for the pomodoro.
        ShapeKind::RecordCompact => 34.0,
        _ => 0.0,
    }
}

/// `shape_for`, widened by `robot_extra` (and still centred) with the robot on.
#[cfg(test)]
pub fn shape_with_robot(style: Style, kind: ShapeKind, robot: bool) -> ShapeRect {
    shape_with_robot_sized(style, NotchSize::DEFAULT, kind, robot)
}

/// `shape_with_robot` with the closed island at the user's size.
pub fn shape_with_robot_sized(
    style: Style,
    size: NotchSize,
    kind: ShapeKind,
    robot: bool,
) -> ShapeRect {
    let s = shape_sized(style, size, kind);
    if !robot {
        return s;
    }
    let w = s.w + robot_extra(kind);
    ShapeRect {
        x: (WINDOW_W - w) / 2.0,
        w,
        ..s
    }
}

/// The robot's square at the left of a compact shape, vertically centred.
pub fn robot_slot(s: ShapeRect) -> Rect {
    square(
        s.x + COMPACT_INSET,
        s.y + (s.h - ROBOT_COMPACT) / 2.0,
        ROBOT_COMPACT,
    )
}

/// The idle island: the robot, the clock, both side by side (centred as a
/// group), or nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IdleLayout {
    pub robot: Option<Rect>,
    /// The clock's box; its text is centred in it.
    pub clock: Option<Rect>,
}

/// How far the idle island grows to the left while the cat types on its
/// laptop (dip): the laptop reaches about 11 dip left of the 26 dip robot
/// square (`robot::laptop_rect`). The right edge stays, so the robot and the
/// clock stay exactly where they are on screen (`idle_layout_lead`).
pub const TYPING_LEAD: f32 = 12.0;

/// The idle shape grown on the LEFT only by `TYPING_LEAD`: x reduced and w
/// increased by it, the right edge and everything else unchanged.
pub fn idle_shape_typing(style: Style, size: NotchSize, robot: bool) -> ShapeRect {
    let s = shape_with_robot_sized(style, size, ShapeKind::Idle, robot);
    ShapeRect {
        x: s.x - TYPING_LEAD,
        w: s.w + TYPING_LEAD,
        ..s
    }
}

/// How much of `shape` is the typing lead: how much more it has grown on the
/// left than on the right of `plain` (the plain idle shape), 0 to
/// `TYPING_LEAD` (NaN is 0). It follows the shape through the idle <-> typing
/// morph, so the group stays still, and is 0 through a symmetric morph (from
/// or to the compact views, a notice or the panel), so the group stays
/// centred there as before.
pub fn typing_lead(shape: ShapeRect, plain: ShapeRect) -> f32 {
    let left = plain.x - shape.x;
    let right = (shape.x + shape.w) - (plain.x + plain.w);
    let lead = left - right;
    if lead.is_nan() {
        0.0
    } else {
        lead.clamp(0.0, TYPING_LEAD)
    }
}

pub fn idle_layout(s: ShapeRect, robot: bool, clock: bool) -> IdleLayout {
    idle_layout_lead(s, robot, clock, 0.0)
}

/// `idle_layout` for a shape grown on the left by `lead` (`typing_lead`): the
/// group (the robot alone, or robot, gap and clock) is centred in the shape
/// minus that lead. A clock alone still fills the whole shape.
pub fn idle_layout_lead(s: ShapeRect, robot: bool, clock: bool, lead: f32) -> IdleLayout {
    let full = Rect {
        x: s.x,
        y: s.y,
        w: s.w,
        h: s.h,
    };
    let y = s.y + (s.h - ROBOT_COMPACT) / 2.0;
    match (robot, clock) {
        (false, clock) => IdleLayout {
            robot: None,
            clock: clock.then_some(full),
        },
        (true, false) => IdleLayout {
            robot: Some(square(
                s.x + lead + (s.w - lead - ROBOT_COMPACT) / 2.0,
                y,
                ROBOT_COMPACT,
            )),
            clock: None,
        },
        (true, true) => {
            let group = ROBOT_COMPACT + ROBOT_GAP + CLOCK_W;
            let x = s.x + lead + (s.w - lead - group) / 2.0;
            IdleLayout {
                robot: Some(square(x, y, ROBOT_COMPACT)),
                clock: Some(Rect {
                    x: x + ROBOT_COMPACT + ROBOT_GAP,
                    y: s.y,
                    w: CLOCK_W,
                    h: s.h,
                }),
            }
        }
    }
}

/// Compact media with the robot in the album art's place (`art` is the
/// robot's square): title, then the equalizer.
pub fn compact_layout_robot(s: ShapeRect) -> CompactLayout {
    let robot = robot_slot(s);
    let plain = compact_layout(s);
    CompactLayout {
        art: robot,
        text: text_box(robot.right() + ROBOT_GAP, plain.bars.x - 10.0, s.y, s.h),
        bars: plain.bars,
    }
}

/// The lyric view with the robot: its square, then the line's box.
pub fn lyric_layout_robot(s: ShapeRect) -> (Rect, Rect) {
    let robot = robot_slot(s);
    let line = lyric_layout(s);
    let left = robot.right() + ROBOT_GAP;
    (robot, text_box(left, line.right(), s.y, s.h))
}

/// The face notch's face ("Jadi muka") keeps this far in from the shape's
/// ends: past the rounded corners, the Pill's top ones too.
pub const FACE_SIDE: f32 = 7.0;
/// Room kept round the face's box (`cat_style::face_half`), in dip: the 1 dip
/// edge of its pieces and a little air.
pub const FACE_MARGIN: f32 = 1.5;
/// How much wider the narrow closed views (media, lyric, pomodoro, REC and
/// its caption, the screen countdown) are in Face mode, on top of
/// `robot_extra`: the face takes the robot's 26 dip slot and needs more, so
/// it is never smaller than the shrunk pill's (`face_scale` >= 1). The
/// shape grows evenly (it stays centred); its content is laid out
/// `FACE_EXTRA` further in from the left (`face_content`), so the face gets
/// all of it.
pub const FACE_EXTRA: f32 = 12.0;

/// The width Face mode adds to a `kind` shape: `FACE_EXTRA` for the narrow
/// closed views, 0 for the idle island (the face and the clock share it,
/// `face_idle_layout`) and everything else. Plain and Gradient never add it.
pub fn face_extra(kind: ShapeKind) -> f32 {
    match kind {
        ShapeKind::MediaCompact
        | ShapeKind::MediaLyric
        | ShapeKind::PomodoroCompact
        | ShapeKind::RecordCompact => FACE_EXTRA,
        _ => 0.0,
    }
}

/// `s` (the view's shape as Plain and Gradient build it: `shape_with_robot_sized`,
/// `lyric_shape_sized`, `with_rec_caption`) widened by `face_extra(kind)`,
/// still centred: the Face-mode shape.
pub fn with_face_extra(s: ShapeRect, kind: ShapeKind) -> ShapeRect {
    let extra = face_extra(kind);
    ShapeRect {
        x: s.x - extra / 2.0,
        w: s.w + extra,
        ..s
    }
}

/// The shape a Face-mode view's content is laid out in (pass it to
/// `compact_layout_robot`, `lyric_layout_robot`, the pomodoro and REC
/// layouts): the Face-mode shape `s` less `face_extra(kind)` on the left, so
/// the content keeps its room and the face gets the extra.
pub fn face_content(s: ShapeRect, kind: ShapeKind) -> ShapeRect {
    let extra = face_extra(kind);
    ShapeRect {
        x: s.x + extra,
        w: s.w - extra,
        ..s
    }
}

/// The least room kept between a text box and the unread dot on the face
/// notch wearing the earcups (dip).
const TEXT_AIR: f32 = 0.5;

/// How much narrower `face_content_wearing` makes the content of a `kind`
/// view for `accessory` than `face_content` (the earcups' and the unread
/// dot's room at the right end).
pub fn face_content_trim(kind: ShapeKind, accessory: crate::theme::CatAccessory) -> f32 {
    if accessory != crate::theme::CatAccessory::Headphone || face_extra(kind) == 0.0 {
        return 0.0;
    }
    // The dot's place depends on the shape's right end only, so any shape
    // gives the same room.
    let s = ShapeRect { x: 0.0, y: 0.0, w: 100.0, h: 34.0, radius: 0.0 };
    let ((cx, _), r) = face_toast_dot(s, super::robot::face_unit(super::robot::FACE_SCALE), kind);
    let dot_room = s.x + s.w - (cx - r - TOAST_DOT_RING);
    (dot_room + TEXT_AIR - COMPACT_INSET).max(0.0)
}

/// `face_content` for a face wearing `accessory`. With the earcups on a
/// narrow view, the content also ends short of the right earcup and the
/// unread dot moved in past it (`face_toast_dot`), so no text box runs under
/// the dot: every view's text keeps `COMPACT_INSET` from the content's right
/// end, so the content ends by what the dot needs past that, at the face's
/// largest size (`robot::FACE_SCALE`; the face never sets the content's
/// right end, so this does not depend on it).
pub fn face_content_wearing(
    s: ShapeRect,
    kind: ShapeKind,
    accessory: crate::theme::CatAccessory,
) -> ShapeRect {
    let c = face_content(s, kind);
    ShapeRect {
        w: (c.w - face_content_trim(kind, accessory)).max(0.0),
        ..c
    }
}

/// The box the face notch's face is centred in (its eyes on the box's
/// centre) in a narrow view: left of `clock_left` (the left of whatever sits
/// right of the face: the media title, the lyric line, the pomodoro ring,
/// the REC dot, as laid out in `face_content`) by the clock's gap,
/// `FACE_SIDE` in from the shape's left end, as tall as the shape. No wider
/// than the full-size face needs, so it stays by what follows it. With no
/// clock it is centred in the shape. Zero wide when there is no room
/// (mid-morph). The idle island uses `face_idle_layout`.
pub fn face_area(shape: Rect, clock_left: Option<f32>) -> Rect {
    use super::cat_style::FACE_HALF_W;
    use super::robot::{FACE_SCALE, face_unit};
    let widest = 2.0 * (FACE_HALF_W * face_unit(FACE_SCALE) + FACE_MARGIN);
    let left = shape.x + FACE_SIDE;
    let (l, r) = match clock_left {
        Some(c) => {
            let r = c - ROBOT_GAP;
            ((r - widest).max(left), r)
        }
        None => {
            let r = shape.right() - FACE_SIDE;
            let w = (r - left).clamp(0.0, widest);
            let mid = (left + r) / 2.0;
            (mid - w / 2.0, mid + w / 2.0)
        }
    };
    Rect {
        x: l,
        y: shape.y,
        w: (r - l).max(0.0),
        h: shape.h,
    }
}

/// The idle face notch: the face's box (`face_area`'s kind, its eyes on the
/// centre) and the clock's box, side by side with the clock's gap and centred
/// as one group in `s`, as `idle_layout` centres the head and the clock. The
/// face is as large as `face_scale` allows in what the clock leaves; without
/// the clock it is centred alone. With the earcups on, the group moves left
/// of centre only as far as the unread dot (`face_toast_dot`) needs to sit
/// clear of the clock, on the narrowest notches.
pub fn face_idle_layout(
    s: ShapeRect,
    clock: bool,
    accessory: crate::theme::CatAccessory,
) -> (Rect, Option<Rect>) {
    use super::robot::face_unit;
    let clock_w = if clock { ROBOT_GAP + CLOCK_W } else { 0.0 };
    let room = Rect {
        x: s.x,
        y: s.y,
        w: (s.w - 2.0 * FACE_SIDE - clock_w).max(0.0),
        h: s.h,
    };
    let scale = face_scale(room, accessory);
    let (half_w, _) = super::cat_style::face_half(accessory);
    let face_w = if scale > 0.0 {
        2.0 * (half_w * face_unit(scale) + FACE_MARGIN)
    } else {
        0.0
    };
    let mut x = s.x + (s.w - face_w - clock_w) / 2.0;
    if clock && accessory == crate::theme::CatAccessory::Headphone && scale > 0.0 {
        let ((cx, _), r) = face_toast_dot(s, face_unit(scale), ShapeKind::Idle);
        let room = cx - r - TOAST_DOT_RING - TEXT_AIR;
        x -= (x + face_w + clock_w - room).max(0.0);
    }
    let face = Rect {
        x,
        y: s.y,
        w: face_w,
        h: s.h,
    };
    let clock = clock.then_some(Rect {
        x: x + face_w + ROBOT_GAP,
        y: s.y,
        w: CLOCK_W,
        h: s.h,
    });
    (face, clock)
}

/// The scale to draw the face at in `area` (`face_area`, `face_idle_layout`)
/// wearing `accessory`: `robot::FACE_SCALE` where it has the room, else the
/// largest at which the face's box (`cat_style::face_half`), its margin and
/// the dance's nod fit; 0 when nothing fits. The corner pieces never count:
/// they sit outside the face's box.
pub fn face_scale(area: Rect, accessory: crate::theme::CatAccessory) -> f32 {
    use super::robot::{FACE_SCALE, NOD_MAX_DIP, face_unit};
    let (half_w, half_h) = super::cat_style::face_half(accessory);
    let across = (area.w / 2.0 - FACE_MARGIN) / half_w;
    let down = (area.h / 2.0 - NOD_MAX_DIP - FACE_MARGIN) / half_h;
    // A hair over, so a box built for the full-size face gives it back.
    let scale = across.min(down) / face_unit(1.0) + 1e-4;
    if scale.is_nan() {
        0.0
    } else {
        scale.clamp(0.0, FACE_SCALE)
    }
}

/// Linear blend; `t` outside 0..=1 extrapolates, which is how the eased
/// overshoot carries past the target shape.
pub fn lerp_shape(a: ShapeRect, b: ShapeRect, t: f32) -> ShapeRect {
    let mix = |a: f32, b: f32| a + (b - a) * t;
    ShapeRect {
        x: mix(a.x, b.x),
        y: mix(a.y, b.y),
        w: mix(a.w, b.w),
        h: mix(a.h, b.h),
        radius: mix(a.radius, b.radius),
    }
}

/// Ease-out with a slight overshoot (peaks about 2 % past the target).
pub fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 0.7;
    const C3: f32 = C1 + 1.0;
    // Exact at the start: f32 rounding would otherwise leave t = 0 a hair off 0.
    if t <= 0.0 {
        return 0.0;
    }
    let u = t - 1.0;
    1.0 + C3 * u * u * u + C1 * u * u
}

/// Depth of the Notch's inward-curved top shoulders, drawn outside `w`.
pub const SHOULDER: f32 = 12.0;

/// The shoulders' depth for `s`: `SHOULDER`, or half the height of a shape
/// shorter than two of them (the notch shrunk to the eyes, and the morph to
/// it), so the bottom corners keep half the height and it stays a fully
/// rounded tab. Every closed and open shape (28 dip and taller) keeps 12.
pub fn shoulder_depth(s: ShapeRect) -> f32 {
    SHOULDER.min(s.h / 2.0).max(0.0)
}

/// Corner radius actually drawn: the stored radius clamped so mid-morph
/// (and overshooting) shapes never produce a self-crossing outline.
pub fn corner_radius(style: Style, s: ShapeRect) -> f32 {
    let vertical = match style {
        Style::Pill => s.h / 2.0,
        Style::Notch => s.h - shoulder_depth(s),
    };
    s.radius.min(s.w / 2.0).min(vertical).max(0.0)
}

/// Whether a logical point lies inside the main shape (shoulders excluded).
pub fn shape_contains(style: Style, s: ShapeRect, x: f32, y: f32) -> bool {
    if x < s.x || x >= s.x + s.w || y < s.y || y >= s.y + s.h {
        return false;
    }
    let r = corner_radius(style, s);
    // Corner circle centers; a point beyond one of them on both axes must lie
    // within that corner's circle.
    let cx = if x < s.x + r {
        s.x + r
    } else if x > s.x + s.w - r {
        s.x + s.w - r
    } else {
        return true;
    };
    let top_rounded = style == Style::Pill;
    let cy = if top_rounded && y < s.y + r {
        s.y + r
    } else if y > s.y + s.h - r {
        s.y + s.h - r
    } else {
        return true;
    };
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// How far past the outline (logical px) a point still counts as the island:
/// the edge's anti-aliased pixels are drawn, so Windows hands them the
/// pointer too.
pub const AA_MARGIN: f32 = 1.0;

/// Whether a logical point lies on the drawn island: the main shape, the
/// Notch's top shoulders, and `margin` around both (the anti-aliased edge).
pub fn island_contains(style: Style, s: ShapeRect, x: f32, y: f32, margin: f32) -> bool {
    let grown = ShapeRect {
        x: s.x - margin,
        y: s.y - margin,
        w: s.w + 2.0 * margin,
        h: s.h + 2.0 * margin,
        radius: corner_radius(style, s) + margin,
    };
    shape_contains(style, grown, x, y) || (style == Style::Notch && in_shoulder(s, x, y, margin))
}

/// A Notch shoulder: the square beside the shape's top corner, outside the
/// circle its inward curve follows (centred `shoulder_depth` out and down).
fn in_shoulder(s: ShapeRect, x: f32, y: f32, margin: f32) -> bool {
    let d = shoulder_depth(s);
    if y < s.y - margin || y > s.y + d {
        return false;
    }
    let (left, right) = (s.x, s.x + s.w);
    let cx = if x >= left - d - margin && x <= left {
        left - d
    } else if x >= right && x <= right + d + margin {
        right + d
    } else {
        return false;
    };
    let cy = s.y + d;
    (x - cx).powi(2) + (y - cy).powi(2) >= (d - margin).powi(2)
}

/// What a client point is over, for the pointer's own messages.
#[derive(Clone, Debug, PartialEq)]
pub enum Under {
    /// A drawn pixel of the island, and what it does when clicked.
    Island(Hit),
    /// Only the faint drag margin: it takes a drag, nothing else.
    Halo,
    /// Nothing the island drew.
    Off,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// Compact media: album art, artist, equalizer bars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompactLayout {
    pub art: Rect,
    pub text: Rect,
    pub bars: Rect,
}

/// Media page: big album art; beside it a caption, title over artist, and
/// the transport buttons with "Lirik" at the row's right end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpandedLayout {
    pub art: Rect,
    /// "SEDANG DIPUTAR" above the title.
    pub label: Rect,
    pub title: Rect,
    pub artist: Rect,
    /// Prev, play/pause (the round primary button), next.
    pub buttons: [Rect; 3],
    /// "Lirik": lyrics in the notch on or off.
    pub lyrics: Rect,
    /// The round button left of "Lirik" that opens the floating lyrics card.
    /// Always laid out; its hit region and its paint only exist while the
    /// lyrics setting is on (`panel_layout::media_hits_for`).
    pub lyrics_card: Rect,
}

/// Notice: rounded badge, title over body, an optional action chip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoticeLayout {
    pub badge: Rect,
    pub title: Rect,
    pub body: Rect,
    pub action: Option<Rect>,
}

/// Equalizer (the design's `.eq`): 5 bars, 2.5 px wide, 2 px apart, up to
/// 13 px tall.
pub const BAR_COUNT: usize = 5;
pub const BAR_W: f32 = 2.5;
pub const BAR_GAP: f32 = 2.0;
pub const BAR_MAX_H: f32 = 13.0;

/// Equalizer bar heights `t` seconds into playback: each bar follows
/// `BAR_MAX_H × (0.35 + 0.65 × |sin(2π·f·t + φ)|)`. Paused bars rest at the
/// middle of that range. Every `f × 10 s` is a whole number of cycles, so the
/// render clock can wrap at 10 s without a jump.
pub fn equalizer_heights(t: f32, playing: bool) -> [f32; BAR_COUNT] {
    const FREQ: [f32; BAR_COUNT] = [1.7, 2.3, 1.3, 2.9, 2.1];
    const PHASE: [f32; BAR_COUNT] = [0.0, 1.1, 2.3, 0.6, 1.7];
    std::array::from_fn(|i| {
        let wave = if playing {
            (std::f32::consts::TAU * FREQ[i] * t + PHASE[i]).sin().abs()
        } else {
            0.5
        };
        BAR_MAX_H * (0.35 + 0.65 * wave)
    })
}

/// How low a bar rests when the music is silent or muted, as a share of
/// `BAR_MAX_H`.
pub const EQ_REST: f32 = 0.2;

/// Equalizer bar heights from the live band levels (`eq_audio`, each 0..=1):
/// a bar rests at `EQ_REST` of `BAR_MAX_H` and rises to the full height with
/// its level. A level that is not a number counts as 0; a muted output keeps
/// every bar at rest.
pub fn audio_bar_heights(levels: [f32; BAR_COUNT], muted: bool) -> [f32; BAR_COUNT] {
    levels.map(|level| {
        let level = if muted || level.is_nan() {
            0.0
        } else {
            level.clamp(0.0, 1.0)
        };
        BAR_MAX_H * (EQ_REST + (1.0 - EQ_REST) * level)
    })
}

/// Text box between `left` and `right`; never negative while a shape is still small.
fn text_box(left: f32, right: f32, y: f32, h: f32) -> Rect {
    Rect {
        x: left,
        y,
        w: (right - left).max(0.0),
        h,
    }
}

fn square(x: f32, y: f32, size: f32) -> Rect {
    Rect {
        x,
        y,
        w: size,
        h: size,
    }
}

pub fn compact_layout(s: ShapeRect) -> CompactLayout {
    const ART: f32 = 22.0;
    const PAD_L: f32 = COMPACT_INSET;
    const PAD_R: f32 = COMPACT_INSET;
    const GAP: f32 = 10.0;
    let art = square(s.x + PAD_L, s.y + (s.h - ART) / 2.0, ART);
    let bars_w = BAR_COUNT as f32 * BAR_W + (BAR_COUNT - 1) as f32 * BAR_GAP;
    let bars = Rect {
        x: s.x + s.w - PAD_R - bars_w,
        y: s.y + (s.h - BAR_MAX_H) / 2.0,
        w: bars_w,
        h: BAR_MAX_H,
    };
    let text = text_box(art.right() + GAP, bars.x - GAP, s.y, s.h);
    CompactLayout { art, text, bars }
}

/// The lyric line's box: the shape less its side padding (wider than the
/// corner radius, so the text never runs into the rounded ends).
pub fn lyric_layout(s: ShapeRect) -> Rect {
    text_box(s.x + LYRIC_PAD, s.x + s.w - LYRIC_PAD, s.y, s.h)
}

/// The lyric notch is as wide as its line, between these (logical px).
pub const LYRIC_MIN_W: f32 = 220.0;
pub const LYRIC_MAX_W: f32 = 560.0;
/// On a small screen the notch takes at most this share of its width.
pub const LYRIC_SCREEN_SHARE: f32 = 0.45;
/// A new line this much shorter (or more) shrinks the notch; less keeps it,
/// so it does not wobble on every line.
pub const LYRIC_SHRINK_SLACK: f32 = 40.0;
/// A width change while the lyric view stays.
pub const LYRIC_RESIZE_MS: u64 = 180;
/// A line too long for the widest notch waits this long, then scrolls.
pub const LYRIC_HOLD_MS: u64 = 1000;
/// How fast a long line scrolls.
pub const LYRIC_SCROLL_PX_PER_S: f32 = 30.0;
/// The faded strip at the right of a line running past its box. The scroll
/// stops with the line's end just left of it.
pub const LYRIC_FADE_W: f32 = 32.0;
/// The line's box: this far from the shape's sides (no robot).
const LYRIC_PAD: f32 = 20.0;
/// Room for the measure's rounding, so a measured line never trims.
const LYRIC_SLACK: f32 = 1.0;

/// How much farther right the line's box starts with the robot since the
/// robot's inset became `COMPACT_INSET` (it was 6): 10. `lyric_width_min` adds
/// it to the width, so the box keeps the room it had.
pub const LYRIC_ROBOT_EXTRA: f32 = COMPACT_INSET - 6.0;

/// Where the line's box starts, from the shape's left edge: after the side
/// padding, or after the robot (`lyric_layout_robot`).
fn lyric_lead(robot: bool) -> f32 {
    if robot {
        COMPACT_INSET + ROBOT_COMPACT + ROBOT_GAP
    } else {
        LYRIC_PAD
    }
}

/// The widest lyric notch on a screen `screen_w` logical px wide.
pub fn lyric_max_w(screen_w: f32) -> f32 {
    (LYRIC_SCREEN_SHARE * screen_w).clamp(LYRIC_MIN_W, LYRIC_MAX_W)
}

/// The notch's width for a line measured `text_w` px wide (`None`: the
/// instrumental rest "♪" or nothing, which takes the minimum).
#[cfg(test)]
pub fn lyric_width(text_w: Option<f32>, robot: bool, max_w: f32) -> f32 {
    lyric_width_min(text_w, robot, LYRIC_MIN_W, max_w)
}

/// `lyric_width` with the minimum raised to the user's notch width (never
/// below `LYRIC_MIN_W`); the maximum is never below the minimum.
pub fn lyric_width_min(text_w: Option<f32>, robot: bool, min_w: f32, max_w: f32) -> f32 {
    let min_w = min_w.max(LYRIC_MIN_W);
    let max_w = max_w.max(min_w);
    // The width the line wanted before the robot's inset grew, clamped as
    // always; the robot's extra lead goes on top, outside the clamp, so the
    // box is never narrower than it was (even at the minimum or maximum).
    let (lead, extra) = if robot {
        (lyric_lead(true) - LYRIC_ROBOT_EXTRA, LYRIC_ROBOT_EXTRA)
    } else {
        (lyric_lead(false), 0.0)
    };
    let base = match text_w {
        Some(w) if w > 0.0 => (w.ceil() + LYRIC_SLACK + lead + LYRIC_PAD).clamp(min_w, max_w),
        _ => min_w,
    };
    base + extra
}

/// The widest `lyric_width_min` gives for this floor and cap: the cap (never
/// under the floor), plus the robot's extra lead with the robot.
pub fn lyric_cap(robot: bool, min_w: f32, max_w: f32) -> f32 {
    let extra = if robot { LYRIC_ROBOT_EXTRA } else { 0.0 };
    max_w.max(min_w.max(LYRIC_MIN_W)) + extra
}

/// The side of the small logo that stands for a busy toast beside a compact
/// view (`docs/memory/toast-while-busy.md`).
pub const TOAST_SLOT: f32 = 20.0;
/// The gap between a compact view's last element and the slot's logo.
pub const TOAST_SLOT_GAP: f32 = 8.0;
/// How much wider a compact shape is while the slot shows: the logo and its
/// gap.
pub const TOAST_SLOT_EXTRA: f32 = TOAST_SLOT + TOAST_SLOT_GAP;
/// The unread dot's diameter.
pub const TOAST_DOT: f32 = 6.0;

/// `s` widened by `REC_CAPTION_EXTRA`, still centred: the REC shape with room
/// for the caption line.
pub fn with_rec_caption(s: ShapeRect) -> ShapeRect {
    ShapeRect {
        x: s.x - REC_CAPTION_EXTRA / 2.0,
        w: s.w + REC_CAPTION_EXTRA,
        ..s
    }
}

/// `s` widened by `TOAST_SLOT_EXTRA`, still centred: the compact shape with
/// room for the logo at its right.
pub fn with_toast_slot(s: ShapeRect) -> ShapeRect {
    ShapeRect {
        x: s.x - TOAST_SLOT_EXTRA / 2.0,
        w: s.w + TOAST_SLOT_EXTRA,
        ..s
    }
}

/// What a lyric notch `w` wide (as `lyric_width_min` gave it) becomes with
/// the slot: the line's side shrinks only when `w` would not leave the
/// logo's room under `lyric_cap`, so the total never passes the cap.
pub fn lyric_slot_width(w: f32, robot: bool, min_w: f32, max_w: f32) -> f32 {
    let cap = lyric_cap(robot, min_w, max_w);
    w.min(cap - TOAST_SLOT_EXTRA).max(0.0) + TOAST_SLOT_EXTRA
}

/// The parts of a compact `shape` that shows (or is morphing to or from) the
/// toast slot: the rect the compact content is laid out on, the logo's
/// square, and the logo's alpha. `base_w` is the width of the content with
/// the slot settled (the shape's width without the slot; for a lyric line
/// that hit its cap, `lyric_slot_width` less `TOAST_SLOT_EXTRA`). The slot's share `e` of the width is what `shape` has over
/// `base_w` (0 to `TOAST_SLOT_EXTRA`, so an overshooting morph is clamped);
/// the content gives up exactly that much of its right side, so nothing under
/// the logo moves at any frame. The logo appears only once it clears the
/// content (`e >= TOAST_SLOT`), fully at `TOAST_SLOT_EXTRA`.
pub fn toast_slot_parts(shape: ShapeRect, base_w: f32) -> (ShapeRect, Rect, f32) {
    let e = (shape.w - base_w).clamp(0.0, TOAST_SLOT_EXTRA);
    let content = ShapeRect {
        w: shape.w - e,
        ..shape
    };
    let logo = square(
        shape.x + shape.w - COMPACT_INSET - TOAST_SLOT,
        shape.y + (shape.h - TOAST_SLOT) / 2.0,
        TOAST_SLOT,
    );
    let alpha = ((e - TOAST_SLOT) / TOAST_SLOT_GAP).clamp(0.0, 1.0);
    (content, logo, alpha)
}

/// How far inside the full toast's sides a pointer must be for a peek: the
/// morph to it may overshoot narrower than its settled width for a frame.
pub const TOAST_PEEK_INSET: f32 = 8.0;

/// A pointer at the logical point (x, y), on the slot, may peek: the full
/// toast it would turn into (the Notice shape, less `TOAST_PEEK_INSET` on each
/// side) still lies under it. Asked of the pointer itself, not of the logo:
/// a compact or lyric shape wider than the toast has a band at its right end
/// where the peek would leave the pointer outside the toast, and the notch
/// would flap between the slot and the toast under a still pointer.
pub fn toast_peek_reaches(style: Style, size: NotchSize, x: f32, y: f32) -> bool {
    let n = shape_sized(style, size, ShapeKind::Notice);
    let inner = ShapeRect {
        x: n.x + TOAST_PEEK_INSET,
        w: (n.w - 2.0 * TOAST_PEEK_INSET).max(0.0),
        ..n
    };
    shape_contains(style, inner, x, y)
}

/// The unread dot's centre and radius: 8 dip in from the right end, at the
/// shape's vertical centre.
pub fn toast_dot(shape: ShapeRect) -> ((f32, f32), f32) {
    (
        (shape.x + shape.w - 8.0, shape.y + shape.h / 2.0),
        TOAST_DOT / 2.0,
    )
}

/// The width of the ring round the unread dot where it reads under 3:1 on
/// the fill, drawn just outside the dot: it reaches this far past it.
pub const TOAST_DOT_RING: f32 = 1.5;

/// How far the face, and the earcups with it, slides sideways in a `kind`
/// view at `unit` dip per grid unit: `robot::face_reach_x` on the idle
/// notch, the only one that looks or glances (`app.rs`, `Spot::Idle`); 0
/// elsewhere (the nod moves it down only).
pub fn face_side_reach(kind: ShapeKind, unit: f32) -> f32 {
    if kind == ShapeKind::Idle {
        super::robot::face_reach_x(unit)
    } else {
        0.0
    }
}

/// The unread dot on Jadi muka's face notch of a `kind` view wearing the
/// earcups, the face at `unit` dip per grid unit: `toast_dot` moved in past
/// the right earcup, so the dot and its ring keep 1 dip clear of the cup
/// wherever it slides with the face (`face_side_reach`,
/// `cat_style::face_cup_left`).
pub fn face_toast_dot(shape: ShapeRect, unit: f32, kind: ShapeKind) -> ((f32, f32), f32) {
    let reach = face_side_reach(kind, unit);
    let ((cx, cy), r) = toast_dot(shape);
    let cup = super::cat_style::face_cup_left(
        Rect {
            x: shape.x,
            y: shape.y,
            w: shape.w,
            h: shape.h,
        },
        unit,
        reach,
    );
    ((cx.min(cup - 1.0 - r - TOAST_DOT_RING), cy), r)
}

/// A toast's strip in the open panel: its 16 dip logo and the text box
/// (`strip` is `panel_layout::waiting_strip`).
pub fn toast_strip_parts(strip: Rect) -> (Rect, Rect) {
    const LOGO: f32 = 16.0;
    let logo = square(strip.x + 6.0, strip.y + (strip.h - LOGO) / 2.0, LOGO);
    let text = Rect {
        x: strip.x + 28.0,
        y: strip.y,
        w: (strip.w - 28.0 - 10.0).max(0.0),
        h: strip.h,
    };
    (logo, text)
}

/// The line's box width in a lyric notch `w` wide.
pub fn lyric_box_w(w: f32, robot: bool) -> f32 {
    (w - lyric_lead(robot) - LYRIC_PAD).max(0.0)
}

/// Hysteresis: the width to show after `current` (`None` when the lyric
/// view just opened) for a line that wants `wanted`. Grows at once; shrinks
/// only by `LYRIC_SHRINK_SLACK` or more.
pub fn lyric_target_w(current: Option<f32>, wanted: f32) -> f32 {
    match current {
        Some(c) if wanted < c && c - wanted < LYRIC_SHRINK_SLACK => c,
        _ => wanted,
    }
}

/// The lyric notch `w` wide, centred, as tall and round as the compact shapes.
#[cfg(test)]
pub fn lyric_shape(style: Style, w: f32) -> ShapeRect {
    lyric_shape_sized(style, NotchSize::DEFAULT, w)
}

/// `lyric_shape` as tall as the user's notch.
pub fn lyric_shape_sized(style: Style, size: NotchSize, w: f32) -> ShapeRect {
    let s = shape_sized(style, size, ShapeKind::MediaLyric);
    ShapeRect {
        x: (WINDOW_W - w) / 2.0,
        w,
        ..s
    }
}

/// A line `text_w` wide runs past its box `box_w` wide.
pub fn lyric_overflow(text_w: f32, box_w: f32) -> bool {
    text_w > box_w
}

/// How far a long line scrolls in all: until its end sits just left of the fade.
fn lyric_scroll_max(text_w: f32, box_w: f32) -> f32 {
    if lyric_overflow(text_w, box_w) {
        text_w - box_w + LYRIC_FADE_W
    } else {
        0.0
    }
}

/// How far the line is scrolled left `ms_since_line` after it showed: 0
/// through the hold, then `LYRIC_SCROLL_PX_PER_S` until its end; 0 for a
/// line that fits.
pub fn lyric_scroll_offset(text_w: f32, box_w: f32, ms_since_line: u64) -> f32 {
    let moving = ms_since_line.saturating_sub(LYRIC_HOLD_MS) as f32 / 1000.0;
    (moving * LYRIC_SCROLL_PX_PER_S).min(lyric_scroll_max(text_w, box_w))
}

/// When, after the line showed, its scroll ends; `None` when it fits.
pub fn lyric_scroll_end_ms(text_w: f32, box_w: f32) -> Option<u64> {
    let max = lyric_scroll_max(text_w, box_w);
    (max > 0.0).then(|| LYRIC_HOLD_MS + (max / LYRIC_SCROLL_PX_PER_S * 1000.0).ceil() as u64)
}

/// The chip naming what a click does ("Selesai", "Mulai", "Buka").
pub const ACTION_W: f32 = 66.0;
/// The Mole notices' "Log" chip: narrower, so their longer titles keep
/// the room ("Mole selesai · 12,4 GB dibebaskan").
pub const LOG_ACTION_W: f32 = 46.0;
/// The failed update's "Lihat rilis" / "See releases" chip: a longer word.
pub const RELEASES_ACTION_W: f32 = 88.0;

/// How wide `kind`'s chip is (`notice_action` says whether it has one).
pub fn notice_action_w(kind: &NoticeKind) -> f32 {
    match kind {
        NoticeKind::MoleDone | NoticeKind::MoleFailed => LOG_ACTION_W,
        NoticeKind::UpdateFailed => RELEASES_ACTION_W,
        NoticeKind::RecSaved {
            offer: TranscriptOffer::Setup,
            ..
        } => SETUP_CHIP_W,
        NoticeKind::RecSaved { .. } => TRANSCRIBE_CHIP_W,
        _ => ACTION_W,
    }
}

/// The update offer's "Update" chip (the primary one, at the right).
pub const UPDATE_CHIP_W: f32 = 56.0;
/// The update offer's "Nanti" / "Later" chip, left of "Update".
pub const LATER_CHIP_W: f32 = 48.0;
/// A sticky note reminder's "Tunda 10 mnt" chip, left of its "Buka" one (which
/// is as wide as the update offer's primary chip).
pub const SNOOZE_CHIP_W: f32 = 88.0;
/// The "Rekaman tersimpan" notice's "Transkrip" / "Transcribe" chip.
pub const TRANSCRIBE_CHIP_W: f32 = 76.0;
/// The same notice's "Pengaturan" / "Settings" chip, when the transcript
/// still needs setting up.
pub const SETUP_CHIP_W: f32 = 84.0;
/// Space between a notice's chips.
pub const NOTICE_CHIP_GAP: f32 = 6.0;

/// The widths of `kind`'s chips, left to right (none for a notice a click
/// only dismisses). The update offer has two: "Nanti" then "Update".
pub fn notice_chip_widths(kind: &NoticeKind, pomo: PomoStatus) -> Vec<f32> {
    match kind {
        NoticeKind::UpdateAvailable { .. } => vec![LATER_CHIP_W, UPDATE_CHIP_W],
        NoticeKind::StickyReminder { .. } => vec![SNOOZE_CHIP_W, UPDATE_CHIP_W],
        _ => notice_action(kind, pomo)
            .map(|_| notice_action_w(kind))
            .into_iter()
            .collect(),
    }
}

/// A notice with several chips in a row at the right (`widths`, left to
/// right, `NOTICE_CHIP_GAP` apart), the text stopping short of the first.
/// Returns the layout (its `action` is the whole block of chips) and each
/// chip's rect, in the order of `widths`.
pub fn notice_actions_layout(s: ShapeRect, widths: &[f32]) -> (NoticeLayout, Vec<Rect>) {
    notice_actions_layout_with(s, widths, false)
}

/// `notice_actions_layout` with the room for the ✕ (`notice_layout_with`).
pub fn notice_actions_layout_with(
    s: ShapeRect,
    widths: &[f32],
    close: bool,
) -> (NoticeLayout, Vec<Rect>) {
    if widths.is_empty() {
        return (notice_layout_with(s, None, close), Vec::new());
    }
    let gaps = NOTICE_CHIP_GAP * (widths.len() - 1) as f32;
    let total = widths.iter().sum::<f32>() + gaps;
    let layout = notice_layout_with(s, Some(total), close);
    let block = layout.action.expect("room for the chips was asked for");
    let mut x = block.x;
    let chips = widths
        .iter()
        .map(|&w| {
            let chip = Rect { x, w, ..block };
            x += w + NOTICE_CHIP_GAP;
            chip
        })
        .collect();
    (layout, chips)
}

/// The "Rekaman tersimpan" notice's one chip as a click target: "Transkrip"
/// (`offer` `Transcribe`) or "Pengaturan" (`Setup`). Nothing for `None`.
pub fn rec_saved_regions(chip: Rect, offer: TranscriptOffer) -> Vec<Region> {
    let hit = match offer {
        TranscriptOffer::None => return Vec::new(),
        TranscriptOffer::Transcribe => Hit::RecTranscribeChip,
        TranscriptOffer::Setup => Hit::RecTranscriptSetup,
    };
    vec![Region { rect: chip, hit }]
}

/// The update offer's click targets: "Nanti" and "Update". Anywhere else on
/// the notice (or the strip) is the body (or the strip) and does nothing.
pub fn update_offer_regions(later: Rect, update: Rect) -> Vec<Region> {
    vec![
        Region {
            rect: later,
            hit: Hit::UpdateLater,
        },
        Region {
            rect: update,
            hit: Hit::UpdateNow,
        },
    ]
}

/// A sticky note reminder's click targets: "Tunda 10 mnt" and "Buka".
pub fn sticky_reminder_regions(snooze: Rect, open: Rect) -> Vec<Region> {
    vec![
        Region {
            rect: snooze,
            hit: Hit::StickySnooze,
        },
        Region {
            rect: open,
            hit: Hit::StickyOpen,
        },
    ]
}

/// How far the due reminder's "Selesai" chip's click target grows past the
/// painted chip on every side. Less than `ACTION_GAP`, so it never reaches
/// the text.
pub const NOTICE_CHIP_HIT_MARGIN: f32 = 4.0;

/// The due reminder notice's click target: the painted chip grown by
/// `NOTICE_CHIP_HIT_MARGIN`. Points outside the island's shape never reach
/// it (`HitRegions::hit` tests the shape first). Anywhere else on the notice
/// is the body.
pub fn reminder_done_regions(chip: Rect) -> Vec<Region> {
    let m = NOTICE_CHIP_HIT_MARGIN;
    vec![Region {
        rect: Rect {
            x: chip.x - m,
            y: chip.y - m,
            w: chip.w + 2.0 * m,
            h: chip.h + 2.0 * m,
        },
        hit: Hit::ReminderDone,
    }]
}

/// The ✕ of a timed full notice (docs/memory/notice-close.md): the hit square's
/// side, its right edge's distance from the shape's right edge, the painted
/// circle, the glyph, and the room between the text (or the chip) and the
/// square.
pub const NOTICE_CLOSE_HIT: f32 = 24.0;
pub const NOTICE_CLOSE_INSET: f32 = 8.0;
pub const NOTICE_CLOSE_DISC: f32 = 20.0;
pub const NOTICE_CLOSE_GLYPH: f32 = 9.0;
pub const NOTICE_CLOSE_GAP: f32 = 6.0;

/// The ✕'s hit square: `NOTICE_CLOSE_HIT` wide, `NOTICE_CLOSE_INSET` from the
/// shape's right edge, centred vertically.
pub fn notice_close_rect(s: ShapeRect) -> Rect {
    square(
        s.x + s.w - NOTICE_CLOSE_INSET - NOTICE_CLOSE_HIT,
        s.y + (s.h - NOTICE_CLOSE_HIT) / 2.0,
        NOTICE_CLOSE_HIT,
    )
}

/// The ✕'s click target: one region, `Hit::NoticeClose`.
pub fn notice_close_regions(rect: Rect) -> Vec<Region> {
    vec![Region {
        rect,
        hit: Hit::NoticeClose,
    }]
}

/// `action`: room for a chip this wide naming what a click does at the
/// right, the text stopping short of it.
pub fn notice_layout(s: ShapeRect, action: Option<f32>) -> NoticeLayout {
    notice_layout_with(s, action, false)
}

/// `notice_layout` with the room for the ✕ (`close`): the text and the chip
/// end `NOTICE_CLOSE_GAP` before its square. Kept for a closable notice
/// whether or not the ✕ shows, so nothing moves when the pointer arrives.
pub fn notice_layout_with(s: ShapeRect, action: Option<f32>, close: bool) -> NoticeLayout {
    // The robot takes exactly the badge's place.
    const BADGE: f32 = ROBOT_NOTICE;
    const PAD_L: f32 = 18.0;
    const PAD_R: f32 = 20.0;
    const ACTION_H: f32 = 24.0;
    const ACTION_GAP: f32 = 10.0;
    let badge = square(s.x + PAD_L, s.y + (s.h - BADGE) / 2.0, BADGE);
    let left = badge.right() + 12.0;
    let mid = s.y + s.h / 2.0;
    let edge = if close {
        notice_close_rect(s).x - NOTICE_CLOSE_GAP
    } else {
        s.x + s.w - PAD_R
    };
    let action = action.map(|w| Rect {
        x: edge - w,
        y: mid - ACTION_H / 2.0,
        w,
        h: ACTION_H,
    });
    let right = action.map_or(edge, |a| a.x - ACTION_GAP);
    NoticeLayout {
        badge,
        title: text_box(left, right, mid - 18.0, 18.0),
        body: text_box(left, right, mid + 1.0, 16.0),
        action,
    }
}

/// A notice made from a real Windows toast: the app's name in a small line
/// above the title, so three lines sit beside the badge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToastNoticeLayout {
    pub badge: Rect,
    pub eyebrow: Rect,
    pub title: Rect,
    pub body: Rect,
    pub action: Option<Rect>,
}

/// `notice_layout`'s badge, chip and text edges with the text split in three
/// lines (eyebrow, title, body) around the shape's middle.
// The renderer asks for the ✕'s room itself (`toast_notice_layout_with`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn toast_notice_layout(s: ShapeRect, action: Option<f32>) -> ToastNoticeLayout {
    toast_notice_layout_with(s, action, false)
}

/// `toast_notice_layout` with the room for the ✕ (`notice_layout_with`).
pub fn toast_notice_layout_with(
    s: ShapeRect,
    action: Option<f32>,
    close: bool,
) -> ToastNoticeLayout {
    let base = notice_layout_with(s, action, close);
    let mid = s.y + s.h / 2.0;
    let (left, right) = (base.title.x, base.title.right());
    ToastNoticeLayout {
        badge: base.badge,
        eyebrow: text_box(left, right, mid - 25.0, 13.0),
        title: text_box(left, right, mid - 12.0, 18.0),
        body: text_box(left, right, mid + 7.0, 16.0),
        action: base.action,
    }
}

/// The line above a toast notice's title: the app, and the web site after it
/// when a browser gave one.
pub fn toast_eyebrow(source: &str, site: Option<&str>) -> String {
    match site {
        Some(site) if !site.is_empty() => format!("{source} \u{b7} {site}"),
        _ => source.to_owned(),
    }
}

/// The screenshot preview's size (`.nt.big` around `.shot.lg` in
/// keputusan-v3.1.html p6).
pub const SHOT_W: f32 = 440.0;
pub const SHOT_H: f32 = 156.0;
const SHOT_PAD: f32 = 10.0;
pub const SHOT_THUMB_W: f32 = 230.0;
pub const SHOT_THUMB_H: f32 = 136.0;
/// The thumbnail's corners (`.shot`).
pub const SHOT_THUMB_RADIUS: f32 = 12.0;
const SHOT_GAP: f32 = 12.0;
/// Bata (or the Camera badge) beside the title.
pub const SHOT_ROBOT: f32 = 26.0;
pub const SHOT_BUTTON_H: f32 = 24.0;
const SHOT_BUTTON_GAP: f32 = 6.0;
/// A button's inner left and right padding (`.act`'s 11 px).
pub const SHOT_BUTTON_PAD: f32 = 11.0;

/// Where a `w` × `h` picture draws inside `area`: whole, scaled to fit
/// (down or up) and centred.
pub fn shot_fit(area: Rect, w: f32, h: f32) -> Rect {
    if w <= 0.0 || h <= 0.0 {
        return area;
    }
    let k = (area.w / w).min(area.h / h);
    let (fw, fh) = (w * k, h * k);
    Rect {
        x: area.x + (area.w - fw) / 2.0,
        y: area.y + (area.h - fh) / 2.0,
        w: fw,
        h: fh,
    }
}

/// The preview's click targets for screenshot `id`: the three buttons and
/// the thumbnail (opens the file). Anywhere else on it is `Hit::Body`.
pub fn shot_regions(l: &ShotLayout, id: u64) -> Vec<Region> {
    let hits = [Hit::ShotCopy(id), Hit::ShotKeep(id), Hit::ShotDelete(id)];
    l.buttons
        .iter()
        .zip(hits)
        .map(|(&rect, hit)| Region { rect, hit })
        .chain([Region {
            rect: l.thumb,
            hit: Hit::ShotOpen(id),
        }])
        .collect()
}

/// Where the preview draws: the thumbnail at the left; the right column
/// with the robot, the title and the two-line meta at its top and the three
/// stacked buttons (Salin lagi, Simpan, Hapus) at its bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotLayout {
    pub thumb: Rect,
    pub robot: Rect,
    pub title: Rect,
    /// "1280 × 720 · 245 KB", then "sudah disalin".
    pub meta: [Rect; 2],
    pub buttons: [Rect; 3],
}

pub fn shot_layout(s: ShapeRect) -> ShotLayout {
    let thumb = Rect {
        x: s.x + SHOT_PAD,
        y: s.y + SHOT_PAD,
        w: SHOT_THUMB_W,
        h: SHOT_THUMB_H,
    };
    let col_x = thumb.right() + SHOT_GAP;
    let col_w = (s.x + s.w - SHOT_PAD - col_x).max(0.0);
    let top = thumb.y;
    // Title (17) over two meta lines (14 each), the robot centred on them.
    let block = 17.0 + 2.0 * 14.0;
    let robot = square(col_x, top + (block - SHOT_ROBOT) / 2.0, SHOT_ROBOT);
    let text_x = robot.right() + 8.0;
    let text_w = (col_x + col_w - text_x).max(0.0);
    let line = |y: f32, h: f32| Rect {
        x: text_x,
        y,
        w: text_w,
        h,
    };
    let bottom = thumb.bottom();
    let button = |i: usize| Rect {
        x: col_x,
        y: bottom - (3 - i) as f32 * SHOT_BUTTON_H - (2 - i) as f32 * SHOT_BUTTON_GAP,
        w: col_w,
        h: SHOT_BUTTON_H,
    };
    ShotLayout {
        thumb,
        robot,
        title: line(top, 17.0),
        meta: [line(top + 17.0, 14.0), line(top + 31.0, 14.0)],
        buttons: [button(0), button(1), button(2)],
    }
}

/// What the last frame drew, for mapping clicks back to targets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitRegions {
    pub shape: Option<(Style, ShapeRect)>,
    pub buttons: Option<[Rect; 3]>,
    /// Panel regions from `panel_layout::regions`; checked before `buttons`.
    pub regions: Vec<Region>,
    /// The CLI page's output, where the wheel scrolls instead of paging.
    pub cli_output: Option<Rect>,
    /// The Clipboard page's cards, where the wheel scrolls them.
    pub clip_list: Option<Rect>,
    /// The Notifikasi page's rows, where the wheel scrolls them.
    pub notif_list: Option<Rect>,
    /// The Mixer page's slider tracks, by row: where a press sets a level.
    pub mixer_tracks: Vec<Rect>,
    /// The Devices page's list, where the wheel scrolls it.
    pub devices_list: Option<Rect>,
    /// The Music page's cover: where the wheel turns the volume.
    pub media_art: Option<Rect>,
    /// The cleaner's Mole log, where the wheel scrolls it.
    pub mole_log: Option<Rect>,
    /// The faint drag margin, while it is drawn.
    pub halo: Option<Rect>,
    /// The page dots' hover zone, on a page that shows the dots.
    pub dock: Option<DockZone>,
}

impl HitRegions {
    /// Whether the wheel at this point scrolls the CLI output.
    pub fn over_cli_output(&self, x: f32, y: f32) -> bool {
        self.cli_output.is_some_and(|r| r.contains(x, y))
    }

    /// What the point is over: the island (its shoulders and anti-aliased
    /// edge included), only the drag margin, or nothing drawn.
    pub fn under(&self, x: f32, y: f32) -> Under {
        if let Some(hit) = self.hit(x, y) {
            return Under::Island(hit);
        }
        if self.halo.is_some_and(|r| r.contains(x, y)) {
            Under::Halo
        } else {
            Under::Off
        }
    }

    /// What a click at the point does: `None` off the drawn island (the
    /// drag margin included). The shoulders and the anti-aliased edge are
    /// the island's body.
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let (style, shape) = self.shape?;
        if !shape_contains(style, shape, x, y) {
            return island_contains(style, shape, x, y, AA_MARGIN).then_some(Hit::Body);
        }
        if let Some(region) = self.regions.iter().find(|r| r.rect.contains(x, y)) {
            return Some(region.hit.clone());
        }
        let targets = [Hit::Prev, Hit::PlayPause, Hit::Next];
        let button = self
            .buttons
            .and_then(|b| b.iter().position(|r| r.contains(x, y)));
        Some(button.map_or(Hit::Body, |i| targets[i].clone()))
    }
}

/// Claude's brand orange: the waiting strip's dot.
pub const CLAUDE: u32 = 0xD97757;
/// WhatsApp's brand green.
pub const WHATSAPP: u32 = 0x25D366;
/// How strongly a tinted badge shows its colour over the island.
const BADGE_TINT: f32 = 0.2;

/// A notice badge: a rounded square of `fill` at `alpha`, and either `icon`
/// or the source's first letter in `ink`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BadgeStyle {
    pub fill: u32,
    pub alpha: f32,
    pub ink: u32,
    pub icon: Option<Icon>,
}

/// Reminders on a solid accent with a bell; the island's own notices on a
/// tint of their colour (Claude the accent, battery ochre, pomodoro the
/// colour of `pomo_next`, the phase a click starts); apps by their letter,
/// WhatsApp on its green tint, others on `raised_hi`.
pub fn badge_style(notice: &Notice, p: &Palette, pomo_next: Phase) -> BadgeStyle {
    let tint = |color: u32, icon| BadgeStyle {
        fill: color,
        alpha: BADGE_TINT,
        ink: color,
        icon: Some(icon),
    };
    match notice.kind {
        NoticeKind::ClaudeDone { .. } | NoticeKind::ClaudeWaiting { .. } => {
            tint(p.accent, Icon::Sparkle)
        }
        NoticeKind::BatteryLow => tint(p.ochre, Icon::Battery),
        // Drawn by the robot (low); the badge shows when it is off.
        NoticeKind::SystemLow => tint(p.ochre, Icon::Activity),
        // Drawn by the robot (happy); the badge shows when it is off.
        NoticeKind::CleanDone | NoticeKind::MoleDone => tint(p.sage, Icon::Sparkle),
        // Drawn by the robot (low); the badge shows when it is off.
        NoticeKind::MoleFailed => tint(p.ochre, Icon::Activity),
        // With the robot off: the flash drive, in the accent.
        NoticeKind::DevicePlugged { .. } => tint(p.accent, Icon::Usb),
        // With the robot off: a check, in sage.
        NoticeKind::DeviceSafe => tint(p.sage, Icon::Check),
        // With the robot off: the Susun layout icon, in the accent.
        NoticeKind::Arranged { .. } => tint(p.accent, Icon::Layout),
        // With the robot off: the download arrow, in the accent, for the
        // offer and the progress; ochre for a failure, a sage check when
        // there is only news (up to date, updated).
        NoticeKind::UpdateAvailable { .. } | NoticeKind::UpdateProgress { .. } => {
            tint(p.accent, Icon::Download)
        }
        NoticeKind::UpdateFailed => tint(p.ochre, Icon::Download),
        NoticeKind::UpdateInfo | NoticeKind::WhatsNew => tint(p.sage, Icon::Check),
        // Drawn by the robot; this badge only shows if it is turned off meanwhile.
        NoticeKind::Greeting { .. } | NoticeKind::SleepReminder { .. } => {
            tint(p.accent, Icon::Sparkle)
        }
        // With the robot off: the Clipboard icon, as on the Pintasan tile.
        NoticeKind::Copied => tint(p.accent, Icon::Clipboard),
        // Text and files with the robot off, or an image whose entry is gone.
        NoticeKind::ClipPreview { .. } => tint(p.accent, Icon::Clipboard),
        // With the robot off: the Camera, as on the Pintasan tile.
        NoticeKind::Screenshot { .. } => tint(p.accent, Icon::Camera),
        NoticeKind::ShotSaved { .. } => tint(p.sage, Icon::Download),
        NoticeKind::RecSaved { .. } => tint(p.sage, Icon::Mic),
        NoticeKind::RecFailed { .. } | NoticeKind::RecMicDenied => {
            tint(crate::theme::REC_RED, Icon::Mic)
        }
        NoticeKind::RecFolderBlocked { .. } => tint(crate::theme::REC_RED, Icon::Folder),
        NoticeKind::RecInfo => tint(p.accent, Icon::Mic),
        // The Notes page's icon (a transcript is a text document): sage when
        // ready, the recording-failed red when not, the accent for a word.
        NoticeKind::TranscriptReady { .. } => tint(p.sage, Icon::Note),
        NoticeKind::TranscriptFailed { .. } => tint(crate::theme::REC_RED, Icon::Note),
        NoticeKind::TranscriptInfo
        | NoticeKind::TranscriptStarting { .. }
        | NoticeKind::TranscriptSettings => {
            tint(p.accent, Icon::Note)
        }
        NoticeKind::ShotDeleted => tint(crate::theme::SOFT_RED, Icon::Trash),
        NoticeKind::Pomodoro => tint(p.phase(pomo_next), Icon::Timer),
        NoticeKind::Reminder { .. } | NoticeKind::StickyReminder { .. } => BadgeStyle {
            fill: p.accent,
            alpha: 1.0,
            ink: p.on_accent,
            icon: Some(Icon::Bell),
        },
        NoticeKind::App { .. } if notice.source.to_lowercase().contains("whatsapp") => BadgeStyle {
            icon: None,
            ..tint(WHATSAPP, Icon::Mail)
        },
        NoticeKind::App { .. } => BadgeStyle {
            fill: p.raised_hi,
            alpha: 1.0,
            ink: p.text,
            icon: None,
        },
    }
}

/// The chip naming what a click on the notice does, only where the click
/// does it: a reminder is marked done, a pomodoro notice starts the next
/// phase while that phase still waits (`Ready`). Claude's notices get none:
/// a click only dismisses them, nothing opens.
pub fn notice_action(kind: &NoticeKind, pomo: PomoStatus) -> Option<Key> {
    match kind {
        NoticeKind::Reminder { .. } => Some(Key::MarkDone),
        NoticeKind::Pomodoro if pomo == PomoStatus::Ready => Some(Key::Start),
        // The click opens the panel on the System page.
        NoticeKind::SystemLow => Some(Key::Open),
        // The click opens Explorer on the drive.
        NoticeKind::DevicePlugged { .. } => Some(Key::Open),
        // The click opens the panel on the Clipboard page.
        NoticeKind::ClipPreview { .. } => Some(Key::ClipPreviewChip),
        // The click opens the cleaner on Mole's log.
        NoticeKind::MoleDone | NoticeKind::MoleFailed => Some(Key::MoleLog),
        // The click opens the releases page. (The update offer has two chips
        // of its own, `notice_chip_widths`; its body does nothing.)
        NoticeKind::UpdateFailed => Some(Key::SeeReleases),
        // "Transkrip" queues one, "Pengaturan" opens the card (the body
        // still shows the file).
        NoticeKind::RecSaved {
            offer: TranscriptOffer::Transcribe,
            ..
        } => Some(Key::TrChipTranscribe),
        NoticeKind::RecSaved {
            offer: TranscriptOffer::Setup,
            ..
        } => Some(Key::TrChipSettings),
        _ => None,
    }
}

/// First character of the source, uppercased (empty when the source is).
pub fn badge_letter(source: &str) -> String {
    source
        .trim_start()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(source: &str, kind: NoticeKind) -> Notice {
        Notice {
            key: "k".into(),
            source: source.into(),
            title: "t".into(),
            body: "b".into(),
            kind,
        }
    }

    fn inside(outer: ShapeRect, r: Rect) -> bool {
        r.x >= outer.x
            && r.y >= outer.y
            && r.right() <= outer.x + outer.w
            && r.bottom() <= outer.y + outer.h
    }

    #[test]
    fn panel_centered_both_styles() {
        let p = shape_for(Style::Pill, ShapeKind::Panel);
        assert_eq!(
            p,
            ShapeRect {
                x: 30.0,
                y: 8.0,
                w: 560.0,
                h: 290.0,
                radius: 30.0
            }
        );
        let n = shape_for(Style::Notch, ShapeKind::Panel);
        assert_eq!(
            (n.x, n.y, n.w, n.h, n.radius),
            (30.0, 0.0, 560.0, 290.0, 30.0)
        );
        // The panel fits the window with the Pill's top gap.
        assert!(p.y + p.h <= WINDOW_H);
        assert_eq!(corner_radius(Style::Notch, n), 30.0);
        assert_eq!(corner_radius(Style::Pill, p), 30.0);
    }

    #[test]
    fn tall_panel_fits_the_tall_window_even_overshooting() {
        let peak = (0..=100)
            .map(|i| ease_out_back(i as f32 / 100.0))
            .fold(0.0, f32::max);
        for style in [Style::Pill, Style::Notch] {
            let tall = shape_for(style, ShapeKind::PanelTall);
            assert_eq!(
                (tall.x, tall.w, tall.h, tall.radius),
                (30.0, 560.0, 440.0, 30.0)
            );
            assert!(tall.y + tall.h <= TALL_WINDOW_H);
            // Growing from any shape eases a little past the target.
            for from in [ShapeKind::Idle, ShapeKind::Panel, ShapeKind::Notice] {
                let s = lerp_shape(shape_for(style, from), tall, peak);
                assert!(s.y + s.h <= TALL_WINDOW_H, "{style:?} from {from:?}: {s:?}");
            }
            // The small window still holds the small panel's overshoot.
            let idle = shape_for(style, ShapeKind::Idle);
            let small = lerp_shape(idle, shape_for(style, ShapeKind::Panel), peak);
            assert!(small.y + small.h <= WINDOW_H, "{style:?}: {small:?}");
        }
    }

    #[test]
    fn cleaner_panel_is_taller_and_fits_the_tall_window() {
        use crate::model::Page;
        let peak = (0..=100)
            .map(|i| ease_out_back(i as f32 / 100.0))
            .fold(0.0, f32::max);
        for style in [Style::Pill, Style::Notch] {
            let clean = shape_for(style, ShapeKind::PanelClean);
            assert_eq!((clean.x, clean.w, clean.h), (30.0, 560.0, 352.0));
            let panel = shape_for(style, ShapeKind::Panel);
            assert_eq!(clean.y, panel.y);
            // Taller than the small window: the island grows its canvas.
            assert!(clean.y + clean.h > WINDOW_H);
            for from in [ShapeKind::Idle, ShapeKind::Panel] {
                let s = lerp_shape(shape_for(style, from), clean, peak);
                assert!(s.y + s.h <= TALL_WINDOW_H, "{style:?} from {from:?}: {s:?}");
            }
        }
        assert_eq!(panel_kind(Page::System, false, true), ShapeKind::PanelClean);
        assert_eq!(panel_kind(Page::System, true, false), ShapeKind::Panel);
        assert_eq!(panel_kind(Page::Cli, true, true), ShapeKind::PanelTall);
        assert_eq!(panel_kind(Page::Cli, false, true), ShapeKind::Panel);
        assert_eq!(panel_kind(Page::Media, true, true), ShapeKind::Panel);
    }

    #[test]
    fn idle_matches_v1_compact_size() {
        for style in [Style::Pill, Style::Notch] {
            let idle = shape_for(style, ShapeKind::Idle);
            // MediaCompact is `MEDIA_COMPACT_EXTRA` wider (equal insets);
            // everything else about it is the island's.
            let media = shape_for(style, ShapeKind::MediaCompact);
            assert_eq!((media.w, media.h), (idle.w + MEDIA_COMPACT_EXTRA, idle.h));
            assert_eq!((media.y, media.radius), (idle.y, idle.radius));
            assert_eq!((idle.w, idle.h), (190.0, 34.0));
        }
        assert_eq!(shape_for(Style::Pill, ShapeKind::Idle).radius, 17.0);
        assert_eq!(shape_for(Style::Notch, ShapeKind::Idle).radius, 16.0);
    }

    #[test]
    fn radius_is_clamped_for_small_shapes() {
        let mut s = shape_for(Style::Pill, ShapeKind::Notice);
        s.h = 20.0;
        assert_eq!(corner_radius(Style::Pill, s), 10.0);
        // Notch sides start below the shoulder, so the radius leaves room
        // for it; a shape this short has shoulders half its height.
        assert_eq!(shoulder_depth(s), 10.0);
        assert_eq!(corner_radius(Style::Notch, s), 20.0 - shoulder_depth(s));
        s.h = 28.0;
        assert_eq!(shoulder_depth(s), SHOULDER);
        assert_eq!(corner_radius(Style::Notch, s), 28.0 - SHOULDER);
        let c = shape_for(Style::Pill, ShapeKind::Idle);
        assert_eq!(corner_radius(Style::Pill, c), 17.0);
    }

    #[test]
    fn pill_corners_are_outside_notch_top_corners_inside() {
        let p = shape_for(Style::Pill, ShapeKind::Notice);
        assert!(!shape_contains(Style::Pill, p, p.x + 1.0, p.y + 1.0));
        assert!(shape_contains(Style::Pill, p, p.x + p.w / 2.0, p.y + 1.0));
        assert!(!shape_contains(Style::Pill, p, p.x - 1.0, p.y + p.h / 2.0));
        let n = shape_for(Style::Notch, ShapeKind::Notice);
        assert!(shape_contains(Style::Notch, n, n.x + 1.0, 1.0));
        assert!(!shape_contains(Style::Notch, n, n.x + 1.0, n.h - 1.0));
        assert!(!shape_contains(Style::Notch, n, n.x + n.w / 2.0, n.h + 1.0));
    }

    #[test]
    fn layouts_fit_inside_their_shapes() {
        for style in [Style::Pill, Style::Notch] {
            let c = shape_for(style, ShapeKind::MediaCompact);
            let l = compact_layout(c);
            for r in [l.art, l.text, l.bars] {
                assert!(inside(c, r), "{style:?} compact {r:?}");
            }
            assert_eq!(l.art.w, 22.0);
            assert!(l.text.x >= l.art.right() && l.text.right() <= l.bars.x);
            // 5 × 2.5 px bars with 4 × 2 px gaps, 13 px tall.
            assert_eq!((l.bars.w, l.bars.h), (20.5, 13.0));
            assert!(l.text.w >= 100.0, "{style:?} compact text {:?}", l.text);

            let m = shape_for(style, ShapeKind::MediaLyric);
            let text = lyric_layout(m);
            assert!(inside(m, text), "{style:?} lyric {text:?}");
            // Without a line: the narrowest notch.
            assert_eq!((text.w, text.h), (180.0, 34.0));
            assert!(text.x - m.x >= corner_radius(style, m));

            let n = shape_for(style, ShapeKind::Notice);
            let l = notice_layout(n, None);
            for r in [l.badge, l.title, l.body] {
                assert!(inside(n, r), "{style:?} notice {r:?}");
            }
            assert_eq!((l.badge.w, l.badge.h), (40.0, 40.0));
            assert!(l.title.x >= l.badge.right() && l.title.bottom() <= l.body.y);
            assert_eq!(l.action, None);
            // With an action chip the text stops short of it.
            let a = notice_layout(n, Some(ACTION_W));
            let chip = a.action.expect("action chip");
            assert_eq!(chip.w, 66.0);
            assert!(inside(n, chip), "{style:?} chip {chip:?}");
            assert!(a.title.right() < chip.x && a.body.right() < chip.x);
            assert!(chip.right() <= n.x + n.w - corner_radius(style, n) / 2.0);
            assert_eq!((a.badge, a.title.x), (l.badge, l.title.x));
            // Mole's "Log" chip is narrower: its text box is wider, the rest
            // as every other notice (360 × 76).
            let m = notice_layout(n, Some(LOG_ACTION_W));
            let log = m.action.expect("log chip");
            assert!(inside(n, log) && log.right() == chip.right());
            assert_eq!(m.title.w, a.title.w + ACTION_W - LOG_ACTION_W);
            assert_eq!((n.w, n.h), (360.0, 76.0));
            assert_eq!(notice_action_w(&NoticeKind::MoleDone), LOG_ACTION_W);
            assert_eq!(notice_action_w(&NoticeKind::CleanDone), ACTION_W);
            assert_eq!(notice_action_w(&NoticeKind::SystemLow), ACTION_W);
        }
    }

    #[test]
    fn toast_notice_layout_keeps_three_lines_inside_every_notice_shape() {
        for style in [Style::Pill, Style::Notch] {
            let mut all = sizes().to_vec();
            all.push(NotchSize::DEFAULT);
            for size in all {
                for robot in [false, true] {
                    let s = if robot {
                        shape_with_robot_sized(style, size, ShapeKind::Notice, true)
                    } else {
                        shape_sized(style, size, ShapeKind::Notice)
                    };
                    for action in [None, Some(ACTION_W), Some(LOG_ACTION_W)] {
                        let l = toast_notice_layout(s, action);
                        let base = notice_layout(s, action);
                        let tag = format!("{style:?} {size:?} robot {robot} action {action:?}");
                        for r in [l.badge, l.eyebrow, l.title, l.body] {
                            assert!(inside(s, r), "{tag}: {r:?}");
                        }
                        if let Some(chip) = l.action {
                            assert!(inside(s, chip), "{tag}: chip");
                        }
                        assert!(l.eyebrow.bottom() <= l.title.y, "{tag}");
                        assert!(l.title.bottom() <= l.body.y, "{tag}");
                        assert!(l.eyebrow.h >= 13.0, "{tag}");
                        assert!(l.title.h >= 18.0, "{tag}");
                        assert!(l.body.h >= 16.0, "{tag}");
                        assert_eq!(l.badge, base.badge, "{tag}");
                        assert_eq!(l.action, base.action, "{tag}");
                        for r in [l.eyebrow, l.title, l.body] {
                            assert_eq!(
                                (r.x, r.right()),
                                (base.title.x, base.title.right()),
                                "{tag}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_toast_eyebrow_adds_the_site_only_when_there_is_one() {
        assert_eq!(
            toast_eyebrow("Google Chrome", Some("web.telegram.org")),
            "Google Chrome \u{b7} web.telegram.org"
        );
        assert_eq!(toast_eyebrow("Slack", None), "Slack");
        assert_eq!(toast_eyebrow("Slack", Some("")), "Slack");
    }

    /// Points along the cat head's rounded outline and its ear tips, drawn in
    /// `area`.
    fn head_outline(area: Rect) -> Vec<(f32, f32)> {
        let (h, r) = crate::ui::robot::head_rect(area);
        let mut out = Vec::new();
        for (cx, cy, a0) in [
            (h.x + r, h.y + r, 180.0f32),
            (h.right() - r, h.y + r, 270.0),
            (h.right() - r, h.bottom() - r, 0.0),
            (h.x + r, h.bottom() - r, 90.0),
        ] {
            for i in 0..=8 {
                let a = (a0 + i as f32 * 90.0 / 8.0).to_radians();
                out.push((cx + r * a.cos(), cy + r * a.sin()));
            }
        }
        // The cat's ear tips, the tops of their round strokes, at rest and
        // raised (alert lifts the ears and the head).
        let k = area.w / crate::ui::robot::GRID;
        for (up, bob) in [(0.0, 0.0), (3.0, -1.5)] {
            for side in [crate::ui::robot::Side::Left, crate::ui::robot::Side::Right] {
                let (x, y) = crate::ui::robot::ear(side, up)[1];
                out.push((area.x + x * k, area.y + (y - 2.5 + bob) * k));
            }
        }
        out
    }

    use crate::theme::Mascot;

    /// Whether a face mark is cream (the music note, the z's, the heart):
    /// these stay at the cat's places; only the espresso eyes ride the
    /// mascot's eye lift.
    fn is_cream(part: &crate::ui::robot::Part) -> bool {
        use crate::ui::robot::{Ink, Part};
        match part {
            Part::Dot { ink, .. } => *ink == Ink::Cream,
            Part::Stroke(line) => line.ink() == Ink::Cream,
            Part::Heart { .. } => true,
            Part::Pill { .. } | Part::Mouth { .. } => false,
        }
    }

    /// A grid point as the mascot's eyes carry it: lifted by the mascot's eye
    /// lift when `eyes` (an espresso mark or a piece that rides the eyes).
    fn lifted(m: Mascot, (x, y): (f32, f32), eyes: bool) -> (f32, f32) {
        let lift = if eyes {
            crate::ui::cat_style::anchors(m).eye_lift
        } else {
            0.0
        };
        (x, y + lift)
    }

    /// The grid points of mascot `m`'s body for `expr` and `pose`: the cat's
    /// head and its ears' round strokes (both grown by `grow`, the Black
    /// coat's rim), the falk's egg and crest, the octopus's mantle and arms.
    fn body_outline(
        m: Mascot,
        f: &crate::ui::robot::Face,
        expr: crate::ui::robot::Expr,
        pose: &crate::ui::robot::Pose,
        grow: f32,
    ) -> Vec<(f32, f32)> {
        use crate::ui::mascot::{falk_outline, octopus_outline};
        use crate::ui::robot::{EAR_STROKE, HEAD, Side, ear_drawn};
        match m {
            Mascot::Falk => falk_outline(expr, pose),
            Mascot::Octopus => octopus_outline(expr, pose),
            Mascot::Cat => {
                let (x, y, w, h, r) = HEAD;
                let mut cat = Vec::new();
                for (cx, cy, a0) in [
                    (x + r, y + r, 180.0f32),
                    (x + w - r, y + r, 270.0),
                    (x + w - r, y + h - r, 0.0),
                    (x + r, y + h - r, 90.0),
                ] {
                    for i in 0..=8 {
                        let a = (a0 + i as f32 * 90.0 / 8.0).to_radians();
                        cat.push((cx + (r + grow) * a.cos(), cy + (r + grow) * a.sin()));
                    }
                }
                for side in [Side::Left, Side::Right] {
                    for (x, y) in ear_drawn(side, f.ears) {
                        cat.extend((0..16).map(|i| {
                            let a = (i as f32 * 22.5).to_radians();
                            let r = EAR_STROKE / 2.0 + grow;
                            (x + r * a.cos(), y + r * a.sin())
                        }));
                    }
                }
                cat
            }
        }
    }

    /// `body_outline` at rest, as the idle face draws it in `area`, in px.
    fn rest_outline(m: Mascot, area: Rect) -> Vec<(f32, f32)> {
        use crate::ui::robot::{Expr, GRID, Pose, face};
        if m == Mascot::Cat {
            return head_outline(area);
        }
        let k = area.w / GRID;
        let f = face(Expr::Idle, false);
        body_outline(m, &f, Expr::Idle, &Pose::default(), 0.0)
            .into_iter()
            .map(|(x, y)| (area.x + x * k, area.y + y * k))
            .collect()
    }

    fn intersects(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    #[test]
    fn robot_and_content_share_every_compact_shape() {
        for style in [Style::Pill, Style::Notch] {
            let head_inside = |s: ShapeRect, robot: Rect| {
                for &m in Mascot::ALL {
                    for (x, y) in rest_outline(m, robot) {
                        assert!(
                            shape_contains(style, s, x, y),
                            "{m:?} {style:?} {robot:?} ({x}, {y})"
                        );
                    }
                }
            };
            for (robot, clock) in [(true, true), (true, false), (false, true)] {
                let s = shape_with_robot(style, ShapeKind::Idle, robot);
                let l = idle_layout(s, robot, clock);
                assert_eq!(l.robot.is_some(), robot);
                assert_eq!(l.clock.is_some(), clock);
                if let Some(r) = l.robot {
                    assert_eq!((r.w, r.h), (ROBOT_COMPACT, ROBOT_COMPACT));
                    head_inside(s, r);
                }
                if let (Some(r), Some(c)) = (l.robot, l.clock) {
                    assert!(inside(s, c) && !intersects(r, c), "{style:?} {l:?}");
                    // Side by side, centred as a group.
                    let (left, right) = (r.x - s.x, s.x + s.w - c.right());
                    assert!((left - right).abs() < 0.01);
                }
            }

            let s = shape_with_robot(style, ShapeKind::MediaCompact, true);
            let l = compact_layout_robot(s);
            head_inside(s, l.art);
            for r in [l.text, l.bars] {
                assert!(
                    inside(s, r) && !intersects(l.art, r),
                    "{style:?} media {r:?}"
                );
            }
            assert!(l.text.right() <= l.bars.x);
            assert_eq!(
                l.text.w,
                compact_layout(shape_for(style, ShapeKind::MediaCompact))
                    .text
                    .w
            );

            // The lyric notch counts the robot in its width: the same line
            // gets the same box with or without it.
            for text_w in [None, Some(120.0), Some(300.0), Some(480.0)] {
                let on = lyric_shape(style, lyric_width(text_w, true, LYRIC_MAX_W));
                let off = lyric_shape(style, lyric_width(text_w, false, LYRIC_MAX_W));
                let (robot, text) = lyric_layout_robot(on);
                head_inside(on, robot);
                assert!(inside(on, text) && !intersects(robot, text));
                let plain = lyric_layout(off);
                assert!(inside(off, plain));
                if let Some(t) = text_w {
                    assert!(text.w >= t && plain.w >= t, "{style:?} {t}");
                    // Above the minimum, the robot only adds its own room.
                    if off.w > LYRIC_MIN_W {
                        assert_eq!(text.w, plain.w, "{style:?} {t}");
                    }
                }
            }
            let s = shape_with_robot(style, ShapeKind::MediaLyric, true);
            let (robot, text) = lyric_layout_robot(s);
            head_inside(s, robot);
            assert!(inside(s, text) && !intersects(robot, text));

            let s = shape_with_robot(style, ShapeKind::PomodoroCompact, true);
            let (robot, (cx, cy), r, text) =
                super::super::panel_layout::pomodoro_compact_layout_robot(s);
            head_inside(s, robot);
            let ring = Rect {
                x: cx - r - 1.5,
                y: cy - r - 1.5,
                w: 2.0 * r + 3.0,
                h: 2.0 * r + 3.0,
            };
            for part in [ring, text] {
                assert!(inside(s, part), "{style:?} pomodoro {part:?}");
                assert!(!intersects(robot, part), "{style:?} pomodoro {part:?}");
            }
            assert!(!intersects(ring, text));

            // A notice's robot is its badge: same 40 px square, text after it.
            let n = shape_with_robot(style, ShapeKind::Notice, true);
            let l = notice_layout(n, Some(ACTION_W));
            assert_eq!((l.badge.w, l.badge.h), (ROBOT_NOTICE, ROBOT_NOTICE));
            head_inside(n, l.badge);
            assert!(l.title.x >= l.badge.right());
        }
    }

    #[test]
    fn every_mascot_stays_in_the_notice_badge() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::accessory_corners;
        use crate::ui::robot::{DOZE_NOD_DIP, DOZE_TILT_DEG, Expr, HOP_PX, Pose, face, mark_bounds};
        let mut checked = 0u32;
        for style in [Style::Pill, Style::Notch] {
            for size in sizes() {
                let n = shape_with_robot_sized(style, size, ShapeKind::Notice, true);
                let badge = notice_layout(n, Some(ACTION_W)).badge;
                assert_eq!((badge.w, badge.h), (ROBOT_NOTICE, ROBOT_NOTICE));
                for &m in Mascot::ALL {
                    // The faces notices show: the greeting's Wave swings the
                    // octopus's arm and the falk's crest, the falk hops on a
                    // done notice; the sleep reminder's faces at rest, with
                    // the yawn wide open and at the doze's deepest sink.
                    let sleepy = [Expr::Drowsy, Expr::Yawn, Expr::Doze];
                    for expr in [Expr::Alert, Expr::Wave, Expr::Happy, Expr::Sad, Expr::Sleep, Expr::Idle]
                        .into_iter()
                        .chain(sleepy)
                    {
                        let f = face(expr, false);
                        let waves: &[f32] = if expr == Expr::Wave { &[-1.0, 0.0, 1.0] } else { &[0.0] };
                        let hops: &[f32] = if m == Mascot::Falk { &[0.0, HOP_PX] } else { &[0.0] };
                        let motions: &[Pose] = if sleepy.contains(&expr) {
                            &[
                                Pose::default(),
                                Pose { yawn: Some(1.0), ..Pose::default() },
                                Pose { nod_dip: DOZE_NOD_DIP, tilt_deg: DOZE_TILT_DEG, ..Pose::default() },
                            ]
                        } else {
                            &[Pose::default()]
                        };
                        for &wave in waves {
                            for (&hop, &motion) in hops.iter().flat_map(|h| motions.iter().map(move |p| (h, p))) {
                                let pose = Pose {
                                    waving: expr == Expr::Wave,
                                    wave,
                                    hop_px: hop,
                                    ..motion
                                };
                                let check = |what: &str, p: (f32, f32), mark: bool| {
                                    let (x, y) = drawn(badge, &f, pose, p, mark);
                                    let y = y - hop;
                                    assert!(
                                        shape_contains(style, n, x, y),
                                        "{m:?} {style:?} {size:?} {expr:?} wave {wave} hop {hop} yawn {:?} nod {} {what}: ({x}, {y}) leaves the notice {n:?}", pose.yawn, pose.nod_dip
                                    );
                                };
                                for p in body_outline(m, &f, expr, &pose, 0.0) {
                                    check("body", p, false);
                                    checked += 1;
                                }
                                for part in &f.parts {
                                    let (l, t, r, b) = mark_bounds(part);
                                    for p in [(l, t), (r, t), (r, b), (l, b)] {
                                        check("mark", lifted(m, p, !is_cream(part)), true);
                                        checked += 1;
                                    }
                                }
                                for acc in CatAccessory::ALL {
                                    for &(p, eyes) in &accessory_corners(m, *acc, f.ears) {
                                        check("accessory", lifted(m, p, eyes), eyes);
                                        checked += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 100_000, "{checked} points checked");
    }

    /// A grid point of the cat as `RobotPainter::paint` draws it in `area`
    /// for `face` and `pose`, in px: a face mark (`mark`) first slides by the
    /// eye offset; then the dance's squash round the head's base, the face's
    /// and the look's tilt about (32, 40), the face's bob, the scale, the
    /// head's slide and the nod.
    fn drawn(
        area: Rect,
        face: &crate::ui::robot::Face,
        pose: crate::ui::robot::Pose,
        (gx, gy): (f32, f32),
        mark: bool,
    ) -> (f32, f32) {
        use crate::ui::robot::{SQUASH_H, SQUASH_ORIGIN, SQUASH_W};
        let k = area.w / crate::ui::robot::GRID;
        // The face's own shift (the typing cat) and the glance slide the
        // marks in the grid too, before the squash and the tilt.
        let (gx, gy) = if mark {
            (
                gx + pose.eye_dx_dip / k + face.eye_shift.0 + pose.glance_x,
                gy + face.eye_shift.1 + pose.glance_y,
            )
        } else {
            (gx, gy)
        };
        let (ox, oy) = SQUASH_ORIGIN;
        let gx = ox + (gx - ox) * (1.0 + SQUASH_W * pose.squash);
        let gy = oy + (gy - oy) * (1.0 - SQUASH_H * pose.squash);
        let a = (face.tilt + pose.tilt_deg).to_radians();
        let (dx, dy) = (gx - 32.0, gy - 40.0);
        let (x, y) = (
            32.0 + dx * a.cos() - dy * a.sin(),
            40.0 + dx * a.sin() + dy * a.cos() + face.bob,
        );
        (
            area.x + x * k + pose.head_dx_dip,
            area.y + y * k + pose.nod_dip,
        )
    }

    /// A grid point drawn in `area` but not turned by the face's tilt (the
    /// typing cat's laptop and paws): scaled, then placed like the cat.
    fn drawn_flat(area: Rect, pose: crate::ui::robot::Pose, (gx, gy): (f32, f32)) -> (f32, f32) {
        let k = area.w / crate::ui::robot::GRID;
        (
            area.x + gx * k + pose.head_dx_dip,
            area.y + gy * k + pose.nod_dip,
        )
    }

    /// `drawn` for a point of the music note's marks, which also move by the
    /// pose's `note_dy_dip` (in dip, after the scale).
    fn drawn_note(
        area: Rect,
        face: &crate::ui::robot::Face,
        pose: crate::ui::robot::Pose,
        p: (f32, f32),
    ) -> (f32, f32) {
        let (x, y) = drawn(area, face, pose, p, true);
        (x, y + pose.note_dy_dip)
    }

    #[test]
    fn the_dancing_cat_stays_in_every_compact_shape() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::{RIM_WIDTH, accessory_corners};
        use crate::ui::robot::{
            Expr, NOTE_MARGIN_DIP, Part, Pose, face, mark_bounds, with_note_lift,
        };
        let steps = |min: u32, max: u32, step: usize, default: u32| {
            let mut v: Vec<u32> = (min..=max).step_by(step).chain([default, max]).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let widths = steps(NOTCH_W_MIN, NOTCH_W_MAX, 10, NOTCH_W_DEFAULT);
        let heights = steps(NOTCH_H_MIN, NOTCH_H_MAX, 2, NOTCH_H_DEFAULT);
        assert_eq!((widths.len(), heights.len()), (15, 9));
        let is_note = |part: &&Part| match part {
            Part::Dot { ink, .. } => *ink == crate::ui::robot::Ink::Cream,
            Part::Stroke(line) => line.ink() == crate::ui::robot::Ink::Cream,
            _ => false,
        };
        let mut checked = 0;
        for style in [Style::Pill, Style::Notch] {
            for &w in &widths {
                for &h in &heights {
                    let size = NotchSize::from_px(w, h);
                    // The compact media view, and the lyric view at several widths.
                    let mut cases = vec![(
                        Expr::Music,
                        shape_with_robot_sized(style, size, ShapeKind::MediaCompact, true),
                        None,
                    )];
                    for lw in [LYRIC_MIN_W, 260.0, 340.0, LYRIC_MAX_W] {
                        cases.push((Expr::Sing, lyric_shape_sized(style, size, lw), Some(lw)));
                    }
                    for (expr, s, lyric_w) in cases {
                        let area = match lyric_w {
                            None => compact_layout_robot(s).art,
                            Some(_) => lyric_layout_robot(s).0,
                        };
                        let f = face(expr, false);
                        let notes: Vec<(f32, f32)> = f
                            .parts
                            .iter()
                            .filter(is_note)
                            .flat_map(|part| {
                                let (l, t, r, b) = mark_bounds(part);
                                [(l, t), (r, t), (r, b), (l, b)]
                            })
                            .collect();
                        for amount in [0.0, 1.0] {
                            for rise in [0.0, 0.5, 1.0] {
                                // The falk's crest sways and the octopus's arms
                                // wobble with the beat: sample it every pi/8 once
                                // per amount (the rise only moves the note).
                                let beats: Vec<f32> = if rise == 0.0 {
                                    use std::f32::consts::{FRAC_PI_2, PI, TAU};
                                    let mut v: Vec<f32> =
                                        (0..16).map(|i| i as f32 * PI / 8.0).collect();
                                    // The octopus arms' phase is t/190 ms + i*1.4 + side:
                                    // hit each arm's extremes (pi/2 and 3pi/2) exactly.
                                    for i in 0..3 {
                                        for side in 0..2 {
                                            for target in [FRAC_PI_2, 3.0 * FRAC_PI_2] {
                                                let t_190 = (target - i as f32 * 1.4 - side as f32)
                                                    .rem_euclid(TAU);
                                                v.push(
                                                    t_190 * PI * 190.0
                                                        / crate::ui::robot::BEAT_MS as f32,
                                                );
                                            }
                                        }
                                    }
                                    v
                                } else {
                                    vec![0.0]
                                };
                                for beat in beats {
                                    let pose = Pose {
                                        beat,
                                        ..with_note_lift(
                                            Pose::default().dance(amount, rise, 0.0),
                                            expr,
                                            area,
                                            s.y,
                                        )
                                    };
                                    let what = format!(
                                        "{style:?} {size:?} {expr:?} {lyric_w:?} amount {amount} rise {rise} beat {beat}"
                                    );
                                    // Every note mark's box is inside the shape.
                                    for &p in &notes {
                                        let (x, y) = drawn_note(area, &f, pose, p);
                                        assert!(
                                            y >= s.y + NOTE_MARGIN_DIP - 1e-3
                                                && y <= s.y + s.h
                                                && x >= s.x
                                                && x <= s.x + s.w,
                                            "{what}: note corner ({x}, {y}) leaves {s:?}"
                                        );
                                    }
                                    for &m in Mascot::ALL {
                                        // The body, bare and (the cat) with the Black rim.
                                        let rims: &[f32] =
                                            if m == Mascot::Cat { &[0.0, RIM_WIDTH] } else { &[0.0] };
                                        for &grow in rims {
                                            for p in body_outline(m, &f, expr, &pose, grow) {
                                                let (x, y) = drawn(area, &f, pose, p, false);
                                                assert!(
                                                    shape_contains(style, s, x, y),
                                                    "{m:?} {what} rim {grow}: ({x}, {y}) leaves the shape"
                                                );
                                            }
                                        }
                                        // Every accessory's boxes.
                                        for acc in CatAccessory::ALL {
                                            for &(p, eyes) in &accessory_corners(m, *acc, f.ears) {
                                                let (x, y) =
                                                    drawn(area, &f, pose, lifted(m, p, eyes), eyes);
                                                assert!(
                                                    shape_contains(style, s, x, y),
                                                    "{m:?} {what} {acc:?}: ({x}, {y}) leaves the shape"
                                                );
                                            }
                                        }
                                        checked += 1;
                                    }
                                }
                            }
                        }
                        // The rise only ever lifts the note: higher at 1 than at 0,
                        // and a note under a tall enough notch really rises.
                        let at = |rise| {
                            with_note_lift(Pose::default().dance(0.0, rise, 0.0), expr, area, s.y)
                                .note_dy_dip
                        };
                        assert!(at(1.0) <= at(0.0), "{style:?} {size:?} {expr:?}");
                        if expr == Expr::Music {
                            assert!(at(1.0) < 0.0, "{style:?} {size:?}: the note rises");
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 2 * 15 * 9 * 5 * 2 * (16 + 12 + 1 + 1) * 3);
    }

    #[test]
    fn the_looking_cat_stays_in_the_idle_island() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::accessory_corners;
        use crate::ui::robot::{Expr, GRID, Pose, face, mark_bounds};
        // Every face the idle view shows (`choose_expr`), open-eyed and
        // blinking. Think is not one of them any more (work running gives
        // Typing, checked by `the_typing_cat_stays_in_the_idle_island`).
        let faces = [Expr::Idle, Expr::Happy, Expr::Sad, Expr::Sleep];
        // Every notch size the sliders give, stepped: each 10 px across and
        // 2 px down, the defaults and both ends included.
        let steps = |min: u32, max: u32, step: usize, default: u32| {
            let mut v: Vec<u32> = (min..=max).step_by(step).chain([default, max]).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let widths = steps(NOTCH_W_MIN, NOTCH_W_MAX, 10, NOTCH_W_DEFAULT);
        let heights = steps(NOTCH_H_MIN, NOTCH_H_MAX, 2, NOTCH_H_DEFAULT);
        assert_eq!((widths.len(), heights.len()), (15, 9));
        for style in [Style::Pill, Style::Notch] {
            for &w in &widths {
                for &h in &heights {
                    let size = NotchSize::from_px(w, h);
                    let s = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
                    for clock in [true, false] {
                        let l = idle_layout(s, true, clock);
                        let area = l.robot.expect("the cat is drawn");
                        let k = area.w / GRID;
                        for &m in Mascot::ALL {
                        for (expr, blink) in faces.iter().flat_map(|&e| [(e, false), (e, true)]) {
                            let f = face(expr, blink);
                            // The body: the cat's head and ears' round strokes, the
                            // falk's egg and crest, the octopus's mantle and arms.
                            let cat = body_outline(m, &f, expr, &Pose::default(), 0.0);
                            // Every mark's box corners: the eyes (lifted by the
                            // mascot's eye lift) and what floats beside the head.
                            let marks: Vec<(f32, f32)> = f
                                .parts
                                .iter()
                                .flat_map(|part| {
                                    let (l, t, r, b) = mark_bounds(part);
                                    [(l, t), (r, t), (r, b), (l, b)]
                                        .map(|p| lifted(m, p, !is_cream(part)))
                                })
                                .collect();
                            for v in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                                let pose = Pose::default().looking_as(expr, v);
                                let at = |p, mark| drawn(area, &f, pose, p, mark);
                                for &p in &cat {
                                    let (x, y) = at(p, false);
                                    assert!(
                                        shape_contains(style, s, x, y),
                                        "{m:?} {style:?} {size:?} {expr:?} v {v}: ({x}, {y}) leaves the island"
                                    );
                                }
                                let drawn_points = cat
                                    .iter()
                                    .map(|&p| at(p, false))
                                    .chain(marks.iter().map(|&p| at(p, true)));
                                if let Some(c) = l.clock {
                                    for (x, y) in drawn_points {
                                        assert!(
                                            x < c.x,
                                            "{m:?} {style:?} {size:?} {expr:?} blink {blink} v {v}: ({x}, {y}) reaches the clock at {}",
                                            c.x
                                        );
                                    }
                                }
                            }
                            // Every accessory's boxes (a piece's corners, the ear ones
                            // turned and lifted with their ear, the glasses on the eyes)
                            // under the same pose: inside the island, left of the clock.
                            for acc in CatAccessory::ALL {
                                let corners = accessory_corners(m, *acc, f.ears);
                                for v in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                                    let pose = Pose::default().looking_as(expr, v);
                                    for &(p, eyes) in &corners {
                                        let (x, y) = drawn(area, &f, pose, lifted(m, p, eyes), eyes);
                                        assert!(
                                            shape_contains(style, s, x, y),
                                            "{m:?} {style:?} {size:?} {acc:?} {expr:?} v {v}: ({x}, {y}) leaves the island"
                                        );
                                        if let Some(c) = l.clock {
                                            assert!(
                                                x < c.x,
                                                "{m:?} {style:?} {size:?} {acc:?} {expr:?} v {v}: ({x}, {y}) reaches the clock at {}",
                                                c.x
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        }
                        // The open eyes slide on inside the head, with room to spare.
                        let pose = Pose::default().looking(1.0);
                        let (eye_l, eye_r) =
                            (21.2 - pose.eye_dx_dip / k, 42.8 + pose.eye_dx_dip / k);
                        assert!(eye_l > 11.0 && eye_r < 53.0, "eyes {eye_l}..{eye_r}");
                    }
                }
            }
        }
    }

    /// A closed compact view as the face notch ("Jadi muka") lays it out.
    struct FaceView {
        name: String,
        kind: ShapeKind,
        /// The Face-mode shape (`with_face_extra`).
        shape: ShapeRect,
        /// The left of what sits right of the face: the idle clock
        /// (`face_idle_layout`), the media title, the lyric line, the pomodoro
        /// ring, the REC dot (laid out in `face_content`); `None` for the idle
        /// island without its clock.
        clock: Option<f32>,
        /// The face's box: its eyes sit on the centre.
        area: Rect,
    }

    /// Every closed compact view the face is drawn in, for `accessory`. The
    /// screen countdown uses the REC shape.
    fn face_views(style: Style, size: NotchSize, accessory: crate::theme::CatAccessory) -> Vec<FaceView> {
        use crate::ui::panel_layout::{pomodoro_compact_layout_robot, record_compact_layout};
        let idle = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
        let mut v = Vec::new();
        for (name, clock) in [("idle", true), ("idle without clock", false)] {
            let (area, c) = face_idle_layout(idle, clock, accessory);
            v.push(FaceView {
                name: name.to_string(),
                kind: ShapeKind::Idle,
                shape: idle,
                clock: c.map(|c| c.x),
                area,
            });
        }
        let mut narrow = |name: String, kind: ShapeKind, plain: ShapeRect, left: &dyn Fn(ShapeRect) -> f32| {
            let shape = with_face_extra(plain, kind);
            let clock = Some(left(face_content(shape, kind)));
            v.push(FaceView { name, kind, shape, clock, area: face_area(as_rect(shape), clock) });
        };
        let plain = |kind| shape_with_robot_sized(style, size, kind, true);
        narrow("media".into(), ShapeKind::MediaCompact, plain(ShapeKind::MediaCompact), &|c| {
            compact_layout_robot(c).text.x
        });
        narrow("pomodoro".into(), ShapeKind::PomodoroCompact, plain(ShapeKind::PomodoroCompact), &|c| {
            let ring = pomodoro_compact_layout_robot(c);
            ring.1.0 - ring.2
        });
        let rec = plain(ShapeKind::RecordCompact);
        for (name, s, caption) in [("record", rec, false), ("caption", with_rec_caption(rec), true)] {
            narrow(name.into(), ShapeKind::RecordCompact, s, &|c| {
                let l = record_compact_layout(c, true, caption);
                l.dot.0.0 - l.dot.1
            });
        }
        for lw in [LYRIC_MIN_W, LYRIC_MAX_W] {
            narrow(format!("lyric {lw}"), ShapeKind::MediaLyric, lyric_shape_sized(style, size, lw), &|c| {
                lyric_layout_robot(c).1.x
            });
        }
        v
    }

    /// One coat of each kind of face: plain, calico patches, cheeks and beak,
    /// a beak alone, octopus spots, the Black cat's amber eyes.
    fn face_looks() -> Vec<crate::ui::cat_style::CatLook> {
        use crate::theme::{CatColor, FalkColor};
        use crate::ui::cat_style::CatLook;
        let cat = |color| CatLook { mascot: Mascot::Cat, color, ..CatLook::default() };
        let falk = |falk| CatLook { mascot: Mascot::Falk, falk, ..CatLook::default() };
        vec![
            cat(CatColor::Terracotta),
            cat(CatColor::Calico),
            cat(CatColor::Black),
            falk(FalkColor::Lutino),
            falk(FalkColor::WhiteFace),
            CatLook { mascot: Mascot::Octopus, ..CatLook::default() },
        ]
    }

    fn as_rect(s: ShapeRect) -> Rect {
        Rect { x: s.x, y: s.y, w: s.w, h: s.h }
    }

    fn centre_of(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The pose of `expr` looking `v` of the way across, nodding `nod` dip:
    /// the faces that look slide, the typing face glances.
    fn face_pose(expr: crate::ui::robot::Expr, v: f32, nod: f32) -> crate::ui::robot::Pose {
        use crate::ui::robot::Pose;
        let pose = Pose { nod_dip: nod, ..Pose::default().looking_as(expr, v) };
        if expr.glances() { pose.glancing(v, v) } else { pose }
    }

    #[test]
    fn the_face_stays_inside_every_compact_notch() {
        use crate::theme::CatAccessory;
        use crate::ui::robot::{
            Expr, FaceBoxKind, NOD_MAX_DIP, face_boxes, face_pose_in, face_slide,
        };
        let looks = face_looks();
        let vs: Vec<f32> = (0..=8).map(|i| -1.0 + 0.25 * i as f32).collect();
        let mut checked = 0usize;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for view in face_views(style, size, CatAccessory::None) {
                    let FaceView { name, kind, shape: s, clock, area } = view;
                    let scale = face_scale(area, CatAccessory::None);
                    let centre = centre_of(area);
                    // Never smaller than the shrunk pill's face.
                    assert!(scale >= 1.0, "{style:?} {size:?} {name}: scale {scale}");
                    for &look in &looks {
                        for expr in Expr::ALL {
                            // A face that neither looks nor glances stays centred.
                            let vs: &[f32] = if expr.follows() { &vs } else { &[0.0] };
                            for &v in vs {
                                for nod in [0.0, NOD_MAX_DIP] {
                                    let pose = face_pose_in(
                                        style, s, kind, clock, centre, scale, expr, look,
                                        face_pose(expr, v, nod),
                                    );
                                    let slide = face_slide(&pose, scale);
                                    for blink in [false, true] {
                                        let what = format!(
                                            "{style:?} {size:?} {name} {:?} {expr:?} v {v} nod {nod} blink {blink}",
                                            look.mascot
                                        );
                                        for b in
                                            face_boxes(look, expr, blink, centre, scale, slide, pose.nod_dip)
                                        {
                                            if let Some(c) = clock {
                                                assert!(b.r < c, "{what}: {b:?} reaches the clock at {c}");
                                            }
                                            // Calico patches are clipped to the shape.
                                            if b.kind == FaceBoxKind::Patch {
                                                continue;
                                            }
                                            for (x, y) in [(b.l, b.t), (b.r, b.t), (b.l, b.b), (b.r, b.b)] {
                                                assert!(
                                                    shape_contains(style, s, x, y),
                                                    "{what}: {b:?} leaves {s:?} at ({x}, {y})"
                                                );
                                            }
                                            checked += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 5_000_000, "{checked} boxes checked");
    }

    #[test]
    fn the_idle_face_and_clock_are_centred_and_full_size_on_the_default_notch() {
        use crate::theme::CatAccessory;
        use crate::ui::robot::{Expr, FACE_SCALE, face_pose_in, face_slide};
        for style in [Style::Pill, Style::Notch] {
            // Every size: the face and the clock as one group, centred.
            for size in notch_sizes() {
                let s = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
                for &accessory in CatAccessory::ALL {
                    let (area, clock) = face_idle_layout(s, true, accessory);
                    let clock = clock.expect("the clock");
                    let (left, right) = (area.x - s.x, s.x + s.w - clock.right());
                    // With the earcups, only as far left of centre as the
                    // unread dot needs (`the_toast_dot_clears_the_content_text`):
                    // only on the narrowest notches.
                    let shift = (right - left) / 2.0;
                    let most = if accessory == CatAccessory::Headphone && size.w < 170.0 { 4.5 } else { 0.0 };
                    assert!(
                        shift > -1e-3 && shift < most + 1e-3,
                        "{style:?} {size:?} {accessory:?}: {left} vs {right}"
                    );
                    assert!((clock.x - area.right() - ROBOT_GAP).abs() < 1e-3);
                    let (alone, none) = face_idle_layout(s, false, accessory);
                    assert!(none.is_none());
                    assert!((centre_of(alone).0 - (s.x + s.w / 2.0)).abs() < 1e-3);
                }
            }
            // 190 x 34: full size whatever is worn, and the look is not cut.
            let s = shape_with_robot_sized(style, NotchSize::DEFAULT, ShapeKind::Idle, true);
            for &accessory in CatAccessory::ALL {
                let (area, _) = face_idle_layout(s, true, accessory);
                assert_eq!(face_scale(area, accessory), FACE_SCALE, "{style:?} {accessory:?}");
            }
            let (area, clock) = face_idle_layout(s, true, CatAccessory::None);
            let clock = clock.map(|c| c.x);
            let centre = centre_of(area);
            for look in face_looks() {
                for v in [-1.0, 1.0] {
                    let want = face_pose(Expr::Idle, v, 0.0);
                    let got = face_pose_in(style, s, ShapeKind::Idle, clock, centre, FACE_SCALE, Expr::Idle, look, want);
                    assert_eq!(face_slide(&got, FACE_SCALE), face_slide(&want, FACE_SCALE), "{style:?} {look:?} v {v}");
                    assert!(face_slide(&got, FACE_SCALE).0.abs() > 0.0);
                }
            }
        }
    }

    #[test]
    fn face_mode_widens_only_the_narrow_views() {
        for kind in [
            ShapeKind::MediaCompact,
            ShapeKind::MediaLyric,
            ShapeKind::PomodoroCompact,
            ShapeKind::RecordCompact,
        ] {
            assert_eq!(face_extra(kind), FACE_EXTRA);
        }
        for kind in [ShapeKind::Idle, ShapeKind::Notice, ShapeKind::Panel, ShapeKind::Eyes] {
            assert_eq!(face_extra(kind), 0.0);
        }
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for kind in [ShapeKind::Idle, ShapeKind::MediaCompact, ShapeKind::PomodoroCompact, ShapeKind::RecordCompact] {
                    let plain = shape_with_robot_sized(style, size, kind, true);
                    let face = with_face_extra(plain, kind);
                    let content = face_content(face, kind);
                    // Still centred, wider by the extra; the content keeps its
                    // width and the right end, the face gets the rest.
                    assert!((face.x + face.w / 2.0 - (plain.x + plain.w / 2.0)).abs() < 1e-3);
                    assert!((face.w - plain.w - face_extra(kind)).abs() < 1e-3);
                    assert!((content.w - plain.w).abs() < 1e-3);
                    assert!((content.x + content.w - (face.x + face.w)).abs() < 1e-3);
                    assert_eq!((face.y, face.h, face.radius), (plain.y, plain.h, plain.radius));
                }
            }
        }
    }

    #[test]
    fn every_face_accessory_stays_inside_the_notch() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::{FACE_HALF_W, FaceAnchor, face_pieces, face_wears};
        use crate::ui::robot::face_unit;
        let mut checked = 0usize;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for &accessory in CatAccessory::ALL {
                    for view in face_views(style, size, accessory) {
                        let FaceView { name, kind, shape: s, clock, area } = view;
                        let scale = face_scale(area, accessory);
                        let unit = face_unit(scale);
                        assert!(scale >= 1.0, "{style:?} {size:?} {name} {accessory:?}: scale {scale}");
                        let centre = centre_of(area);
                        let pieces = face_pieces(accessory, kind, as_rect(s), centre, unit);
                        let worn = accessory != CatAccessory::None && face_wears(accessory, kind);
                        assert_eq!(!pieces.is_empty(), worn, "{name} {accessory:?}");
                        // The narrow views wear the glasses, the bow tie, the
                        // earcups and the hats' brims, never the corner pieces
                        // or the scarf.
                        if kind != ShapeKind::Idle {
                            assert!(!matches!(
                                accessory,
                                CatAccessory::Scarf | CatAccessory::Bow | CatAccessory::Flower
                            ) || pieces.is_empty());
                        }
                        // The hats are brims in every view.
                        let hat = matches!(
                            accessory,
                            CatAccessory::Nightcap | CatAccessory::Crown | CatAccessory::Sailor
                        );
                        assert_eq!(
                            hat,
                            !pieces.is_empty() && pieces.iter().all(|p| p.anchor == FaceAnchor::Rim),
                            "{style:?} {size:?} {name} {accessory:?}"
                        );
                        // The corner pieces: left of the face, and still visible.
                        let face_left = centre.0 - FACE_HALF_W * unit;
                        let corner: Vec<(f32, f32)> = pieces
                            .iter()
                            .filter(|p| p.anchor == FaceAnchor::Corner)
                            .flat_map(|p| p.corners((0.0, 0.0), 0.0))
                            .collect();
                        if !corner.is_empty() {
                            let (lo, hi) = corner
                                .iter()
                                .fold((f32::MAX, f32::MIN), |a, &(x, _)| (a.0.min(x), a.1.max(x)));
                            assert!(hi < face_left, "{style:?} {size:?} {name} {accessory:?}: {hi} reaches the face at {face_left}");
                            assert!(hi - lo >= 8.0, "{style:?} {size:?} {name} {accessory:?}: {} dip wide", hi - lo);
                        }
                        // At rest; moving with the face, see
                        // `the_face_and_its_pieces_stay_inside_when_they_move`.
                        for p in &pieces {
                            let what = format!("{style:?} {size:?} {name} {accessory:?}: {p:?}");
                            // The edge pieces run past the outline (it clips
                            // them), but each shows: the middle of what of its
                            // box lies across the notch is in.
                            if p.anchor.hugs_edge() {
                                let c = p.corners((0.0, 0.0), 0.0);
                                let (l, t, r, b) = c.iter().fold(
                                    (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                                    |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)),
                                );
                                let (t, b) = (t.max(s.y), b.min(s.y + s.h));
                                let (mx, my) = ((l + r) / 2.0, (t + b) / 2.0);
                                assert!(shape_contains(style, s, mx, my), "{what}: its middle ({mx}, {my}) is out");
                            }
                            for (x, y) in p.corners((0.0, 0.0), 0.0) {
                                assert!(
                                    p.anchor.hugs_edge() || shape_contains(style, s, x, y),
                                    "{what} leaves {s:?} at ({x}, {y})"
                                );
                                // The earcups and the scarf belong to the
                                // notch's ends and bottom edge, not the face.
                                if let (Some(c), false) = (clock, p.anchor == FaceAnchor::Notch) {
                                    assert!(x < c, "{what} reaches the clock at {c}");
                                }
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 20_000, "{checked} pieces checked");
    }

    /// The eyes' offset (dip) when the face slides `slide` and nods `nod`, as
    /// `robot::face_boxes` draws them.
    fn eyes_offset(centre: (f32, f32), scale: f32, slide: (f32, f32), nod: f32) -> (f32, f32) {
        use crate::ui::robot::Expr;
        let rest = eye_boxes(Expr::Idle, false, centre, scale, (0.0, 0.0), 0.0);
        let moved = eye_boxes(Expr::Idle, false, centre, scale, slide, nod);
        (moved[0].0 - rest[0].0, moved[0].1 - rest[0].1)
    }

    /// Jadi muka: the face and everything it wears move as one, like a head.
    /// Every piece of every accessory, on every face view, moves by exactly
    /// the eyes' offset: the look's slide plus the nod.
    #[test]
    fn every_worn_piece_moves_with_the_eyes() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::{CatLook, face_pieces};
        use crate::ui::robot::{Expr, NOD_MAX_DIP, face_pose_in, face_slide, face_unit};
        let mut checked = 0usize;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for &accessory in CatAccessory::ALL {
                    let look = CatLook { accessory, ..CatLook::default() };
                    for view in face_views(style, size, accessory) {
                        let FaceView { name, kind, shape: s, clock, area } = view;
                        let scale = face_scale(area, accessory);
                        let centre = centre_of(area);
                        let pieces = face_pieces(accessory, kind, as_rect(s), centre, face_unit(scale));
                        // The pose the island draws (cut back), and two raw ones.
                        let pose = face_pose_in(
                            style, s, kind, clock, centre, scale, Expr::Idle, look,
                            face_pose(Expr::Idle, 0.5, NOD_MAX_DIP),
                        );
                        let moves = [
                            (face_slide(&pose, scale), pose.nod_dip),
                            ((1.5, -0.75), 0.0),
                            ((-2.0, 1.0), NOD_MAX_DIP),
                        ];
                        for (slide, nod) in moves {
                            let (ox, oy) = eyes_offset(centre, scale, slide, nod);
                            for p in &pieces {
                                let rest = p.corners((0.0, 0.0), 0.0);
                                for (a, b) in rest.iter().zip(p.corners(slide, nod)) {
                                    let (mx, my) = (b.0 - a.0, b.1 - a.1);
                                    assert!(
                                        (mx - ox).abs() < 1e-3 && (my - oy).abs() < 1e-3,
                                        "{style:?} {size:?} {name} {accessory:?} slide {slide:?} nod {nod}: \
                                         {p:?} moves ({mx}, {my}), the eyes ({ox}, {oy})"
                                    );
                                }
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 10_000, "{checked} pieces checked");
    }

    /// One clamp for the group: wherever the face looks and nods, cut back
    /// by `face_pose_in`, every mark and every whole-object piece (the
    /// glasses, the bow tie, the bow, the flower) stays inside the island and
    /// left of the clock / content. The edge pieces (earcups, scarf, hats)
    /// move too but the outline clips them instead
    /// (`render::tests::edge_pieces_never_paint_outside_the_island`).
    #[test]
    fn the_face_and_its_pieces_stay_inside_when_they_move() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::{CatLook, face_pieces};
        use crate::ui::robot::{
            Expr, FaceBoxKind, NOD_MAX_DIP, face_boxes, face_pose_in, face_slide, face_unit,
        };
        let vs: Vec<f32> = (0..=8).map(|i| -1.0 + 0.25 * i as f32).collect();
        let mut checked = 0usize;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for &accessory in CatAccessory::ALL {
                    let look = CatLook { accessory, ..CatLook::default() };
                    for view in face_views(style, size, accessory) {
                        let FaceView { name, kind, shape: s, clock, area } = view;
                        let scale = face_scale(area, accessory);
                        let centre = centre_of(area);
                        let pieces = face_pieces(accessory, kind, as_rect(s), centre, face_unit(scale));
                        for expr in Expr::ALL {
                            let vs: &[f32] = if expr.follows() { &vs } else { &[0.0] };
                            for &v in vs {
                                for nod in [0.0, NOD_MAX_DIP] {
                                    let pose = face_pose_in(
                                        style, s, kind, clock, centre, scale, expr, look,
                                        face_pose(expr, v, nod),
                                    );
                                    let slide = face_slide(&pose, scale);
                                    let what = format!(
                                        "{style:?} {size:?} {name} {accessory:?} {expr:?} v {v} nod {nod}"
                                    );
                                    for p in pieces.iter().filter(|p| !p.anchor.hugs_edge()) {
                                        for (x, y) in p.corners(slide, pose.nod_dip) {
                                            assert!(
                                                shape_contains(style, s, x, y),
                                                "{what}: {p:?} leaves {s:?} at ({x}, {y})"
                                            );
                                            if let Some(c) = clock {
                                                assert!(x < c, "{what}: {p:?} reaches the clock at {c}");
                                            }
                                        }
                                        checked += 1;
                                    }
                                    for blink in [false, true] {
                                        for b in face_boxes(look, expr, blink, centre, scale, slide, pose.nod_dip) {
                                            if b.kind == FaceBoxKind::Patch {
                                                continue;
                                            }
                                            if let Some(c) = clock {
                                                assert!(b.r < c, "{what} blink {blink}: {b:?} reaches the clock at {c}");
                                            }
                                            for (x, y) in [(b.l, b.t), (b.r, b.t), (b.l, b.b), (b.r, b.b)] {
                                                assert!(
                                                    shape_contains(style, s, x, y),
                                                    "{what} blink {blink}: {b:?} leaves {s:?} at ({x}, {y})"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 300_000, "{checked} pieces checked");
    }

    /// Without a whole-object piece the face keeps its full look, glance and
    /// nod on the 190 x 34 notch, whatever edge piece it wears: the earcups,
    /// the scarf and the hats never cut it.
    #[test]
    fn the_edge_pieces_never_cut_the_faces_move() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::CatLook;
        use crate::ui::robot::{Expr, NOD_MAX_DIP, Pose, face_pose_in, face_slide};
        let worn = [
            CatAccessory::None,
            CatAccessory::Scarf,
            CatAccessory::Headphone,
            CatAccessory::Nightcap,
            CatAccessory::Crown,
            CatAccessory::Sailor,
        ];
        let mut checked = 0;
        for style in [Style::Pill, Style::Notch] {
            for accessory in worn {
                let look = CatLook { accessory, ..CatLook::default() };
                for view in face_views(style, NotchSize::DEFAULT, accessory) {
                    let FaceView { name, kind, shape: s, clock, area } = view;
                    let scale = face_scale(area, accessory);
                    let centre = centre_of(area);
                    let mut poses = vec![
                        (Expr::Music, Pose { nod_dip: NOD_MAX_DIP, ..Pose::default() }),
                        (Expr::Idle, Pose { nod_dip: NOD_MAX_DIP, ..Pose::default().looking(-1.0) }),
                        (Expr::Idle, Pose { nod_dip: NOD_MAX_DIP, ..Pose::default().looking(1.0) }),
                    ];
                    for (vx, vy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                        poses.push((Expr::Typing, Pose::default().glancing(vx, vy)));
                    }
                    for (expr, want) in poses {
                        let got = face_pose_in(style, s, kind, clock, centre, scale, expr, look, want);
                        let (w, g) = (face_slide(&want, scale), face_slide(&got, scale));
                        let what = format!("{style:?} {name} {accessory:?} {expr:?} {want:?}");
                        assert!((w.0 - g.0).abs() < 1e-4 && (w.1 - g.1).abs() < 1e-4, "{what}: {g:?} of {w:?}");
                        assert_eq!(got.nod_dip, want.nod_dip, "{what}");
                        checked += 1;
                    }
                    // The idle face's full look: 4 dip across, nodding 1.5.
                    if kind == ShapeKind::Idle {
                        let p = face_pose_in(
                            style, s, kind, clock, centre, scale, Expr::Idle, look,
                            Pose { nod_dip: NOD_MAX_DIP, ..Pose::default().looking(-1.0) },
                        );
                        assert_eq!((face_slide(&p, scale).0, p.nod_dip), (-4.0, 1.5), "{style:?} {name} {accessory:?}");
                    }
                }
            }
        }
        assert!(checked > 100, "{checked}");
    }

    /// The hats' brims move with the face but never cut it: at its full
    /// glance right (`face_side_reach`, and nodding), every brim piece, its
    /// edge included, still ends left of the clock / content, on every size,
    /// style and view.
    #[test]
    fn hat_brims_stay_clear_of_the_clock_at_full_glance() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::face_pieces;
        use crate::ui::robot::{NOD_MAX_DIP, face_unit};
        let mut checked = 0;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for hat in [CatAccessory::Nightcap, CatAccessory::Crown, CatAccessory::Sailor] {
                    for view in face_views(style, size, hat) {
                        let FaceView { name, kind, shape: s, clock, area } = view;
                        let Some(c) = clock else {
                            continue;
                        };
                        let unit = face_unit(face_scale(area, hat));
                        let reach = face_side_reach(kind, unit);
                        for p in face_pieces(hat, kind, as_rect(s), centre_of(area), unit) {
                            for (x, _) in p.corners((reach, 0.0), NOD_MAX_DIP) {
                                assert!(x < c, "{style:?} {size:?} {name} {hat:?}: {x} reaches the clock at {c}");
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 1_000, "{checked}");
    }

    /// The scarf's band runs past both ends of the notch by the face's
    /// sideways reach, so sliding with the face never opens a gap at either
    /// end (the outline clips what is past it).
    #[test]
    fn the_scarf_never_shows_a_gap_at_either_end() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::face_pieces;
        use crate::ui::robot::{face_reach_x, face_unit};
        let mut checked = 0;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for view in face_views(style, size, CatAccessory::Scarf) {
                    let FaceView { name, kind, shape: s, area, .. } = view;
                    let unit = face_unit(face_scale(area, CatAccessory::Scarf));
                    let pieces = face_pieces(CatAccessory::Scarf, kind, as_rect(s), centre_of(area), unit);
                    // The band: painted last, over its tail.
                    let Some(band) = pieces.last() else {
                        continue;
                    };
                    let reach = face_reach_x(unit);
                    let c = band.corners((0.0, 0.0), 0.0);
                    let (l, r) = c.iter().fold((f32::MAX, f32::MIN), |a, &(x, _)| (a.0.min(x), a.1.max(x)));
                    assert!(
                        l + reach <= s.x && r - reach >= s.x + s.w,
                        "{style:?} {size:?} {name}: band {l}..{r} on {}..{}",
                        s.x,
                        s.x + s.w
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "{checked}");
    }

    /// The eyes' boxes as drawn ("open and shut" by `blink`), without what
    /// sits beside them: `robot::face_boxes`' eye marks.
    fn eye_boxes(
        expr: crate::ui::robot::Expr,
        blink: bool,
        centre: (f32, f32),
        scale: f32,
        slide: (f32, f32),
        nod: f32,
    ) -> Vec<(f32, f32, f32, f32)> {
        use crate::ui::robot::{face_marks, face_unit, mark_bounds};
        let u = face_unit(scale);
        let at = |x: f32, y: f32| (centre.0 + (x - 32.0) * u + slide.0, centre.1 + (y - 40.0) * u + nod + slide.1);
        face_marks(expr, blink)
            .into_iter()
            // The eyes keep their places; what floats beside them is smaller.
            .filter(|m| m.k == 1.0)
            .map(|m| {
                let (l, t, r, b) = mark_bounds(&m.part);
                let ((l, t), (r, b)) = (at(l + m.at.0, t + m.at.1), at(r + m.at.0, b + m.at.1));
                (l, t, r, b)
            })
            .collect()
    }

    #[test]
    fn face_hat_brims_hug_the_top_edge_and_never_cover_the_eyes() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::{FaceAnchor, face_pieces};
        use crate::ui::robot::{Expr, NOD_MAX_DIP, face_pose_in, face_slide, face_unit};
        let vs = [-1.0, -0.5, 0.0, 0.5, 1.0];
        let mut checked = 0usize;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for hat in [CatAccessory::Nightcap, CatAccessory::Crown, CatAccessory::Sailor] {
                    let look = crate::ui::cat_style::CatLook { accessory: hat, ..Default::default() };
                    for view in face_views(style, size, hat) {
                        let FaceView { name, kind, shape: s, clock, area } = view;
                        let scale = face_scale(area, hat);
                        let unit = face_unit(scale);
                        let centre = centre_of(area);
                        let pieces = face_pieces(hat, kind, as_rect(s), centre, unit);
                        let what = format!("{style:?} {size:?} {name} {hat:?}");
                        assert!(!pieces.is_empty(), "{what}");
                        let boxes: Vec<(f32, f32, f32, f32)> = pieces
                            .iter()
                            .map(|p| {
                                assert_eq!(p.anchor, FaceAnchor::Rim, "{what}");
                                p.corners((0.0, 0.0), 0.0).iter().fold(
                                    (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                                    |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)),
                                )
                            })
                            .collect();
                        // It hugs the notch's top edge, running past it as far
                        // as the face moves down, so it never comes off it...
                        let top = boxes.iter().fold(f32::MAX, |a, b| a.min(b.1));
                        let down = crate::ui::robot::face_reach_down(unit);
                        assert!(top + down <= s.y + 1e-3, "{what}: top {top} vs {}", s.y);
                        // ...over the face: the brim (what starts past the top)
                        // reaches past the face's centre and over the right eye.
                        let (l, r) = boxes
                            .iter()
                            .filter(|b| b.1 < s.y)
                            .fold((f32::MAX, f32::MIN), |a, b| (a.0.min(b.0), a.1.max(b.2)));
                        assert!(l < centre.0 && r > centre.0 + 8.0 * unit, "{what}: brim {l}..{r}");
                        for expr in Expr::ALL {
                            let vs: &[f32] = if expr.follows() { &vs } else { &[0.0] };
                            for &v in vs {
                                for nod in [0.0, NOD_MAX_DIP] {
                                    let pose = face_pose_in(
                                        style, s, kind, clock, centre, scale, expr, look,
                                        face_pose(expr, v, nod),
                                    );
                                    let slide = face_slide(&pose, scale);
                                    // The brim moves with the face.
                                    let boxes: Vec<(f32, f32, f32, f32)> = pieces
                                        .iter()
                                        .map(|p| {
                                            p.corners(slide, pose.nod_dip).iter().fold(
                                                (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                                                |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)),
                                            )
                                        })
                                        .collect();
                                    for blink in [false, true] {
                                        for e in eye_boxes(expr, blink, centre, scale, slide, pose.nod_dip) {
                                            for b in &boxes {
                                                let apart = b.2 <= e.0 || e.2 <= b.0 || b.3 <= e.1 || e.3 <= b.1;
                                                assert!(
                                                    apart,
                                                    "{what} {expr:?} v {v} nod {nod} blink {blink}: {b:?} covers the eye {e:?}"
                                                );
                                                checked += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1_000_000, "{checked} pairs checked");
    }

    #[test]
    fn typing_on_a_face_is_the_idle_eyes() {
        use crate::ui::robot::{Expr, Line, Part, face_marks};
        for blink in [false, true] {
            let idle = face_marks(Expr::Idle, blink);
            let typing = face_marks(Expr::Typing, blink);
            assert!(!idle.is_empty());
            assert_eq!(typing, idle, "blink {blink}");
            // Only the eyes: no laptop, no paws, nothing beside them.
            assert!(typing.iter().all(|m| matches!(
                m.part,
                Part::Pill { .. } | Part::Stroke(Line::Blink)
            )));
        }
    }

    /// Every notch size the sliders give, stepped: each 10 px across and 2 px
    /// down, the defaults and both ends included.
    fn notch_sizes() -> Vec<NotchSize> {
        let steps = |min: u32, max: u32, step: usize, default: u32| {
            let mut v: Vec<u32> = (min..=max).step_by(step).chain([default, max]).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let widths = steps(NOTCH_W_MIN, NOTCH_W_MAX, 10, NOTCH_W_DEFAULT);
        let heights = steps(NOTCH_H_MIN, NOTCH_H_MAX, 2, NOTCH_H_DEFAULT);
        assert_eq!((widths.len(), heights.len()), (15, 9));
        widths
            .iter()
            .flat_map(|&w| heights.iter().map(move |&h| NotchSize::from_px(w, h)))
            .collect()
    }

    #[test]
    fn the_typing_shape_grows_to_the_left_only() {
        assert_eq!(TYPING_LEAD, 12.0);
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                let idle = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
                let typing = idle_shape_typing(style, size, true);
                // The right edge stays; the left edge moves out by 12.
                assert_eq!(typing.x + typing.w, idle.x + idle.w, "{style:?} {size:?}");
                assert_eq!(typing.x, idle.x - TYPING_LEAD);
                assert_eq!(typing.w, idle.w + TYPING_LEAD);
                assert_eq!(
                    (typing.y, typing.h, typing.radius),
                    (idle.y, idle.h, idle.radius)
                );
            }
        }
    }

    #[test]
    fn the_typing_lead_follows_the_shape_and_is_clamped() {
        let plain = ShapeRect {
            x: 100.0,
            y: 0.0,
            w: 190.0,
            h: 32.0,
            radius: 12.0,
        };
        // Grown on the left only by `w`, and moved by `dx` on both sides.
        let left = |w: f32| ShapeRect {
            x: plain.x - w,
            w: plain.w + w,
            ..plain
        };
        let both = |dx: f32| ShapeRect {
            x: plain.x - dx,
            w: plain.w + 2.0 * dx,
            ..plain
        };
        assert_eq!(typing_lead(plain, plain), 0.0);
        assert_eq!(typing_lead(left(12.0), plain), 12.0);
        assert_eq!(typing_lead(left(6.0), plain), 6.0);
        // Past the end (an overshoot), and a symmetric shape wider or
        // narrower than the plain one (a compact view, the eyes).
        assert_eq!(typing_lead(left(25.0), plain), 12.0);
        assert_eq!(typing_lead(both(7.0), plain), 0.0);
        assert_eq!(typing_lead(both(185.0), plain), 0.0);
        assert_eq!(typing_lead(both(-78.0), plain), 0.0);
        // Grown more on the right than on the left.
        assert_eq!(typing_lead(left(-6.0), plain), 0.0);
        let nan = ShapeRect {
            x: f32::NAN,
            ..plain
        };
        assert_eq!(typing_lead(nan, plain), 0.0);
        assert_eq!(typing_lead(plain, nan), 0.0);
    }

    #[test]
    fn the_robot_and_the_clock_stay_centred_back_to_the_plain_idle_island() {
        // A symmetric morph back to the plain idle island (media ends, a
        // notice ends, the panel closes) keeps the group at the plain idle x,
        // as before the typing lead.
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                let idle = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
                let froms = [
                    shape_with_robot_sized(style, size, ShapeKind::MediaCompact, true),
                    shape_with_robot_sized(style, size, ShapeKind::PomodoroCompact, true),
                    lyric_shape_sized(style, size, size.w.max(220.0) + 40.0),
                    shape_with_robot_sized(style, size, ShapeKind::Notice, true),
                    shape_with_robot_sized(style, size, ShapeKind::Panel, true),
                ];
                for from in froms {
                    for clock in [true, false] {
                        let plain = idle_layout(idle, true, clock);
                        for t in [0.0, 0.1, 0.3, 0.5, 0.75, 0.9, 1.0, 1.1] {
                            let shape = lerp_shape(from, idle, t);
                            let lead = typing_lead(shape, idle);
                            let l = idle_layout_lead(shape, true, clock, lead);
                            let (a, b) = (l.robot.unwrap(), plain.robot.unwrap());
                            assert!(
                                (a.x - b.x).abs() < 1e-3,
                                "{style:?} {size:?} from {from:?} clock {clock} t {t}: robot {a:?} vs {b:?}"
                            );
                            if let (Some(c), Some(d)) = (l.clock, plain.clock) {
                                assert!(
                                    (c.x - d.x).abs() < 1e-3,
                                    "{style:?} {size:?} from {from:?} t {t}: clock {c:?} vs {d:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_robot_and_the_clock_stay_still_while_the_island_grows_left() {
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                let idle = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
                let typing = idle_shape_typing(style, size, true);
                for clock in [true, false] {
                    let plain = idle_layout(idle, true, clock);
                    // The two ends and the morph points between (the eased
                    // overshoot only moves the shape past them).
                    for t in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
                        let shape = lerp_shape(idle, typing, t);
                        let lead = typing_lead(shape, idle);
                        let l = idle_layout_lead(shape, true, clock, lead);
                        let (a, b) = (l.robot.unwrap(), plain.robot.unwrap());
                        assert!(
                            (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3,
                            "{style:?} {size:?} clock {clock} t {t}: robot {a:?} vs {b:?}"
                        );
                        if let (Some(c), Some(d)) = (l.clock, plain.clock) {
                            assert!(
                                (c.x - d.x).abs() < 1e-3 && (c.y - d.y).abs() < 1e-3,
                                "{style:?} {size:?} t {t}: clock {c:?} vs {d:?}"
                            );
                        }
                    }
                }
            }
        }
        // No lead is the plain layout, so the Settings preview is unchanged.
        let s = shape_for(Style::Notch, ShapeKind::Idle);
        for (robot, clock) in [(true, true), (true, false), (false, true), (false, false)] {
            assert_eq!(
                idle_layout_lead(s, robot, clock, 0.0),
                idle_layout(s, robot, clock)
            );
        }
    }

    #[test]
    fn the_typing_cat_stays_in_the_idle_island() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::{RIM_WIDTH, accessory_corners};
        use crate::ui::robot::{Expr, PAW_LIFT, PAWS, Pose, face, mark_bounds, typing_points};
        let glances = [
            (-1.0, -1.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (0.0, 0.0),
        ];
        let paws = [[0.0, 0.0], [PAW_LIFT, 0.0], [0.0, PAW_LIFT], [PAW_LIFT, PAW_LIFT]];
        let mut checked = 0u32;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                let s = idle_shape_typing(style, size, true);
                let plain = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
                let lead = typing_lead(s, plain);
                assert_eq!(lead, TYPING_LEAD);
                for clock in [true, false] {
                    let l = idle_layout_lead(s, true, clock, lead);
                    let area = l.robot.expect("the cat is drawn");
                    for blink in [false, true] {
                        let f = face(Expr::Typing, blink);
                        // Everything must be inside the island and left of the
                        // clock box.
                        let check = |what: &str, (x, y): (f32, f32)| {
                            assert!(
                                shape_contains(style, s, x, y),
                                "{style:?} {size:?} clock {clock} blink {blink} {what}: ({x}, {y}) leaves the island {s:?}"
                            );
                            if let Some(c) = l.clock {
                                assert!(
                                    x < c.x,
                                    "{style:?} {size:?} blink {blink} {what}: ({x}, {y}) reaches the clock at {}",
                                    c.x
                                );
                            }
                        };
                        for &m in Mascot::ALL {
                            for (vx, vy) in glances {
                                for lift in paws {
                                    // The octopus's typing arms follow the paw lift.
                                    let pose = Pose {
                                        paw_lift: lift,
                                        ..Pose::default().glancing(vx, vy)
                                    };
                                    let check = |what: &str, (x, y): (f32, f32)| {
                                        check(&format!("{m:?} {what}"), (x, y))
                                    };
                                    // The body, bare and (the cat) with the Black rim.
                                    let rims: &[f32] =
                                        if m == Mascot::Cat { &[0.0, RIM_WIDTH] } else { &[0.0] };
                                    for &grow in rims {
                                        for p in body_outline(m, &f, Expr::Typing, &pose, grow) {
                                            check("body", drawn(area, &f, pose, p, false));
                                            checked += 1;
                                        }
                                    }
                                    // Every eye's box: the pills, or the blink lines.
                                    for part in &f.parts {
                                        let (l, t, r, b) = mark_bounds(part);
                                        for p in [(l, t), (r, t), (r, b), (l, b)] {
                                            let p = lifted(m, p, !is_cream(part));
                                            check("eye", drawn(area, &f, pose, p, true));
                                            checked += 1;
                                        }
                                    }
                                    // Every accessory, the glasses on the shifted and
                                    // glancing eyes.
                                    for acc in CatAccessory::ALL {
                                        for &(p, eyes) in &accessory_corners(m, *acc, f.ears) {
                                            let p = lifted(m, p, eyes);
                                            check("accessory", drawn(area, &f, pose, p, eyes));
                                            checked += 1;
                                        }
                                    }
                                    // The laptop and the paws are not turned by the tilt;
                                    // the falk's feather tips stand on the paws' places.
                                    for p in typing_points(lift) {
                                        check("laptop", drawn_flat(area, pose, p));
                                        checked += 1;
                                    }
                                    if m == Mascot::Falk {
                                        for (i, &(x, y)) in PAWS.iter().enumerate() {
                                            let y = y - lift[i];
                                            for p in [
                                                (x - 4.0, y - 2.6),
                                                (x + 4.4, y - 2.6),
                                                (x + 4.4, y + 2.4),
                                                (x - 4.0, y + 2.4),
                                            ] {
                                                check("tip", drawn_flat(area, pose, p));
                                                checked += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1_000_000, "{checked} points checked");
    }

    #[test]
    fn the_shrunk_notch_is_a_small_pill_that_alone_takes_clicks() {
        for style in [Style::Pill, Style::Notch] {
            let normal = shape_for(style, ShapeKind::Idle);
            let eyes = shape_for(style, ShapeKind::Eyes);
            assert_eq!((eyes.w, eyes.h), (EYES_W, EYES_H));
            assert_eq!((EYES_W, EYES_H), (34.0, 14.0));
            // The same top as the compact shapes, centred like them.
            assert_eq!(eyes.y, normal.y);
            assert_eq!(eyes.x * 2.0 + eyes.w, WINDOW_W);
            // Fully rounded in both styles: the Notch's shoulders give up
            // half the height so the bottom corners keep the other half.
            assert_eq!(corner_radius(style, eyes), EYES_H / 2.0, "{style:?}");
            if style == Style::Notch {
                assert_eq!(shoulder_depth(eyes), EYES_H / 2.0);
                // The bottom corner's arc: off the very corner, on its circle's inside.
                let r = EYES_H / 2.0;
                let (bx, by) = (eyes.x + 0.5, eyes.y + eyes.h - 0.5);
                assert!(!shape_contains(style, eyes, bx, by));
                assert!(shape_contains(
                    style,
                    eyes,
                    eyes.x + r,
                    eyes.y + eyes.h - 0.5
                ));
                // The shoulders still curve in from the screen edge.
                assert!(island_contains(
                    style,
                    eyes,
                    eyes.x - 1.0,
                    eyes.y + 0.5,
                    0.0
                ));
                assert!(!island_contains(
                    style,
                    eyes,
                    eyes.x - 1.0,
                    eyes.y + r - 0.5,
                    0.0
                ));
                // The notch it shrinks from keeps its 12 dip shoulders.
                assert_eq!(shoulder_depth(normal), SHOULDER);
            }
            assert_eq!(
                shape_with_robot(style, ShapeKind::Eyes, true),
                eyes,
                "the robot widens nothing here"
            );
            let regions = HitRegions {
                shape: Some((style, eyes)),
                ..Default::default()
            };
            // On the pill: its body (a press there only restores the notch).
            let (cx, cy) = (eyes.x + eyes.w / 2.0, eyes.y + eyes.h / 2.0);
            assert_eq!(regions.hit(cx, cy), Some(Hit::Body));
            assert_eq!(regions.under(cx, cy), Under::Island(Hit::Body));
            // In the old notch but off the pill: nothing, the click goes to
            // the window below.
            for (x, y) in [
                (normal.x + 10.0, normal.y + normal.h / 2.0),
                (normal.x + normal.w - 10.0, normal.y + normal.h / 2.0),
                (cx, normal.y + normal.h - 3.0),
            ] {
                assert_eq!(regions.hit(x, y), None, "{style:?} ({x}, {y})");
                assert_eq!(regions.under(x, y), Under::Off, "{style:?} ({x}, {y})");
            }
        }
    }

    #[test]
    fn the_shrunk_eyes_stay_inside_the_pill() {
        use crate::pointer_watch::{EYES_LOOK_DOWN_DIP, EYES_LOOK_X_DIP};
        use crate::theme::{FalkColor, Mascot};
        use crate::ui::cat_style::CatLook;
        use crate::ui::peek_face::peek_face;
        use crate::ui::robot::{eyes_boxes_at, eyes_centre, eyes_offset_in};
        // The marks that slide with the eyes: none on the cat, cheeks and a
        // beak on the Lutino falk, the beak alone on the WhiteFace one.
        let looks = [
            CatLook::default(),
            CatLook { mascot: Mascot::Falk, falk: FalkColor::Lutino, ..CatLook::default() },
            CatLook { mascot: Mascot::Falk, falk: FalkColor::WhiteFace, ..CatLook::default() },
        ];
        let steps = |min: u32, max: u32, step: usize, default: u32| {
            let mut v: Vec<u32> = (min..=max).step_by(step).chain([default, max]).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let widths = steps(NOTCH_W_MIN, NOTCH_W_MAX, 10, NOTCH_W_DEFAULT);
        let heights = steps(NOTCH_H_MIN, NOTCH_H_MAX, 2, NOTCH_H_DEFAULT);
        assert_eq!((widths.len(), heights.len()), (15, 9));
        // The look's own extremes, then ones far past them: the clamp's job.
        let (lx, ly) = (EYES_LOOK_X_DIP, EYES_LOOK_DOWN_DIP);
        let offsets = [
            (0.0, 0.0),
            (lx, 0.0),
            (-lx, 0.0),
            (0.0, ly),
            (lx, ly),
            (-lx, ly),
            (20.0, 0.0),
            (-20.0, 0.0),
            (20.0, 10.0),
            (-20.0, 10.0),
            (0.0, 10.0),
        ];
        // Eyes and the beak by their box corners; a cheek (a dot) as the
        // circle it is.
        let inside = |style, s: ShapeRect, boxes: &[(f32, f32, f32, f32)], marks: &[crate::ui::peek_face::PeekMark]| {
            let first_mark = boxes.len() - marks.len();
            boxes.iter().enumerate().all(|(i, &(l, t, r, b))| {
                let at = |x: f32, y: f32| island_contains(style, s, x, y, 0.0);
                match marks.get(i.wrapping_sub(first_mark)) {
                    Some(crate::ui::peek_face::PeekMark::Dot { .. }) => {
                        let (cx, cy, rad) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0);
                        (0..32).all(|k| {
                            let a = k as f32 / 32.0 * std::f32::consts::TAU;
                            at(cx + rad * a.cos(), cy + rad * a.sin())
                        })
                    }
                    _ => [(l, t), (r, t), (r, b), (l, b)].into_iter().all(|(x, y)| at(x, y)),
                }
            })
        };
        for look in looks {
            let marks = peek_face(look, 0).face_marks;
            for style in [Style::Pill, Style::Notch] {
                let pill = shape_sized(style, NotchSize::DEFAULT, ShapeKind::Eyes);
                // The final pill takes the look's extremes as they are: the clamp
                // is a guard, not a visible limit.
                // The cat has no marks to fit, so only it keeps every extreme.
                let kept = if marks.is_empty() { 6 } else { 0 };
                for &(dx, dy) in &offsets[..kept] {
                    assert_eq!(eyes_offset_in(style, pill, dx, dy, &marks), (dx, dy), "{style:?}");
                    for blink in [false, true] {
                        let boxes = eyes_boxes_at(eyes_centre(pill), dx, dy, blink, &marks);
                        assert!(inside(style, pill, &boxes, &marks), "{style:?} ({dx}, {dy}) {blink}");
                    }
                }
                // Past the extremes the pill does cut the slide back (down
                // first, then across what is left of the room).
                let (cx, cy) = eyes_offset_in(style, pill, 20.0, 10.0, &marks);
                assert!(cx >= 0.0 && cx < 20.0 && cy > 0.0 && cy < 10.0, "{style:?} {cx} {cy}");
                if marks.is_empty() {
                    assert!(cx > 0.0 && cy > ly, "{style:?} {cx} {cy}");
                    assert!(eyes_offset_in(style, pill, 20.0, 0.0, &marks).0 > lx);
                }
                for &w in &widths {
                    for &h in &heights {
                        let size = NotchSize::from_px(w, h);
                        let eyes = shape_sized(style, size, ShapeKind::Eyes);
                        assert_eq!(eyes, pill, "the pill does not follow the notch size");
                        let mut normals = Vec::new();
                        for robot in [true, false] {
                            for kind in [
                                ShapeKind::Idle,
                                ShapeKind::MediaCompact,
                                ShapeKind::PomodoroCompact,
                            ] {
                                normals.push(shape_with_robot_sized(style, size, kind, robot));
                            }
                            for lyric_w in [LYRIC_MIN_W.max(size.w), 380.0, LYRIC_MAX_W] {
                                normals.push(lyric_shape_sized(style, size, lyric_w));
                            }
                        }
                        for normal in normals {
                            for step in 0..=10 {
                                let s = lerp_shape(normal, eyes, step as f32 / 10.0);
                                for &(dx, dy) in &offsets {
                                    let (ox, oy) = eyes_offset_in(style, s, dx, dy, &marks);
                                    // Never past what was asked, never the other way.
                                    assert!(ox.abs() <= dx.abs() && ox * dx >= 0.0, "{ox} {dx}");
                                    assert!(oy.abs() <= dy.abs() && oy * dy >= 0.0, "{oy} {dy}");
                                    for blink in [false, true] {
                                        let boxes = eyes_boxes_at(eyes_centre(s), ox, oy, blink, &marks);
                                        assert!(
                                            inside(style, s, &boxes, &marks),
                                            "{style:?} {size:?} t {step}/10 ({dx}, {dy}) -> ({ox}, {oy}) blink {blink} in {s:?}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_falk_looks_a_little_less_far() {
        use crate::pointer_watch::EYES_LOOK_X_DIP;
        use crate::theme::{FalkColor, Mascot};
        use crate::ui::cat_style::CatLook;
        use crate::ui::peek_face::peek_face;
        use crate::ui::robot::{eyes_boxes_at, eyes_centre, eyes_offset_in};
        let look = CatLook { mascot: Mascot::Falk, falk: FalkColor::Lutino, ..CatLook::default() };
        let marks = peek_face(look, 0).face_marks;
        for style in [Style::Pill, Style::Notch] {
            let pill = shape_sized(style, NotchSize::DEFAULT, ShapeKind::Eyes);
            let (x, _) = eyes_offset_in(style, pill, EYES_LOOK_X_DIP, 0.0, &marks);
            // A little shorter than the cat's 4 dip, not cut to a stub.
            assert!(x >= 3.0 && x <= EYES_LOOK_X_DIP, "{style:?} {x}");
            // The cheeks are the two marks after the two eyes: every point of
            // each cheek's circle stays inside the pill.
            let boxes = eyes_boxes_at(eyes_centre(pill), x, 0.0, false, &marks);
            for &(l, t, r, b) in &boxes[2..4] {
                let (cx, cy, rad) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0);
                for k in 0..32 {
                    let a = k as f32 / 32.0 * std::f32::consts::TAU;
                    let (px, py) = (cx + rad * a.cos(), cy + rad * a.sin());
                    assert!(island_contains(style, pill, px, py, 0.0), "{style:?} cheek {k}");
                }
            }
        }
    }

    #[test]
    fn the_screenshot_preview_follows_the_design() {
        for style in [Style::Pill, Style::Notch] {
            let s = shape_with_robot(style, ShapeKind::NoticeShot, true);
            assert_eq!((s.w, s.h, s.radius), (440.0, 156.0, 26.0));
            assert_eq!(s.x * 2.0 + s.w, WINDOW_W, "centred");
            assert!(s.y + s.h <= WINDOW_H, "fits the small canvas");
            let l = shot_layout(s);
            assert_eq!((l.thumb.x - s.x, l.thumb.y - s.y), (10.0, 10.0));
            assert_eq!((l.thumb.w, l.thumb.h), (230.0, 136.0));
            assert!(l.buttons[0].w >= 178.0, "right column");
            assert_eq!(l.buttons[2].bottom(), l.thumb.bottom());
            assert_eq!(l.buttons[0].x, l.thumb.right() + 12.0);
            assert!((l.buttons[0].right() - (s.x + s.w - 10.0)).abs() < 0.01);
            for pair in l.buttons.windows(2) {
                assert!(pair[0].bottom() < pair[1].y, "stacked, apart");
            }
            assert!(l.meta[1].bottom() <= l.buttons[0].y);
            assert_eq!(l.robot.w, 26.0);
            assert!(l.title.x >= l.robot.right() && inside(s, l.title));
            for r in l.buttons.iter().chain([&l.thumb, &l.robot]) {
                assert!(inside(s, *r), "{style:?} {r:?}");
            }
        }
    }

    #[test]
    fn the_screenshot_preview_takes_its_clicks() {
        let s = shape_with_robot(Style::Notch, ShapeKind::NoticeShot, true);
        let l = shot_layout(s);
        let mut regions = HitRegions {
            shape: Some((Style::Notch, s)),
            regions: shot_regions(&l, 7),
            ..HitRegions::default()
        };
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        for (r, hit) in
            l.buttons
                .iter()
                .zip([Hit::ShotCopy(7), Hit::ShotKeep(7), Hit::ShotDelete(7)])
        {
            let (x, y) = mid(*r);
            assert_eq!(regions.hit(x, y), Some(hit));
        }
        let (x, y) = mid(l.thumb);
        assert_eq!(regions.hit(x, y), Some(Hit::ShotOpen(7)));
        // The title is no button: a click there is the notice's own.
        let (x, y) = mid(l.title);
        assert_eq!(regions.hit(x, y), Some(Hit::Body));
        regions.regions.clear();
        assert_eq!(
            regions.hit(mid(l.buttons[2]).0, mid(l.buttons[2]).1),
            Some(Hit::Body)
        );
    }

    #[test]
    fn a_screenshot_fits_its_box_whole() {
        let area = Rect {
            x: 10.0,
            y: 20.0,
            w: 230.0,
            h: 136.0,
        };
        // 16:9 is a little wider than the box: full width, centred down.
        let f = shot_fit(area, 1280.0, 720.0);
        assert!((f.w - 230.0).abs() < 0.01 && f.h < 136.0);
        assert!((f.y - area.y - (136.0 - f.h) / 2.0).abs() < 0.01);
        // A tall one: full height, centred across.
        let f = shot_fit(area, 400.0, 1200.0);
        assert!((f.h - 136.0).abs() < 0.01 && f.w < 230.0 && f.x > area.x);
        assert_eq!(shot_fit(area, 0.0, 10.0), area);
    }

    #[test]
    fn robot_widens_compact_shapes_only_when_on() {
        for style in [Style::Pill, Style::Notch] {
            for kind in [
                ShapeKind::Idle,
                ShapeKind::MediaCompact,
                ShapeKind::MediaLyric,
                ShapeKind::PomodoroCompact,
                ShapeKind::Notice,
                ShapeKind::Panel,
                ShapeKind::PanelTall,
                ShapeKind::PanelClean,
            ] {
                // Off: exactly as before.
                assert_eq!(shape_with_robot(style, kind, false), shape_for(style, kind));
                let on = shape_with_robot(style, kind, true);
                assert_eq!(on.x * 2.0 + on.w, WINDOW_W, "{style:?} {kind:?} centred");
                assert_eq!(
                    (on.y, on.h),
                    (shape_for(style, kind).y, shape_for(style, kind).h)
                );
            }
            let w = |kind| shape_with_robot(style, kind, true).w;
            assert_eq!(w(ShapeKind::Idle), 190.0);
            assert_eq!(w(ShapeKind::MediaCompact), 204.0);
            // Sized by its line, robot included (`lyric_width`).
            assert_eq!(w(ShapeKind::MediaLyric), LYRIC_MIN_W);
            assert_eq!(w(ShapeKind::PomodoroCompact), 246.0);
            assert_eq!(w(ShapeKind::Notice), 360.0);
            assert_eq!(w(ShapeKind::Panel), 560.0, "never in the panel");
        }
    }

    #[test]
    fn hit_maps_buttons_body_and_outside() {
        let e = shape_for(Style::Pill, ShapeKind::Panel);
        let l = super::super::panel_layout::media_panel_layout(e, false);
        let regions = HitRegions {
            shape: Some((Style::Pill, e)),
            buttons: Some(l.buttons),
            ..Default::default()
        };
        let center = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = center(l.buttons[0]);
        assert_eq!(regions.hit(x, y), Some(Hit::Prev));
        let (x, y) = center(l.buttons[1]);
        assert_eq!(regions.hit(x, y), Some(Hit::PlayPause));
        let (x, y) = center(l.buttons[2]);
        assert_eq!(regions.hit(x, y), Some(Hit::Next));
        let (x, y) = center(l.art);
        assert_eq!(regions.hit(x, y), Some(Hit::Body));
        assert_eq!(regions.hit(2.0, 2.0), None);
        assert_eq!(HitRegions::default().hit(x, y), None);
    }

    #[test]
    fn hit_maps_panel_regions() {
        let p = shape_for(Style::Notch, ShapeKind::Panel);
        let strip = Rect {
            x: p.x + 20.0,
            y: 8.0,
            w: 100.0,
            h: 24.0,
        };
        let regions = HitRegions {
            shape: Some((Style::Notch, p)),
            regions: vec![Region {
                rect: strip,
                hit: Hit::Strip,
            }],
            ..Default::default()
        };
        assert_eq!(regions.hit(strip.x + 1.0, strip.y + 1.0), Some(Hit::Strip));
        assert_eq!(regions.hit(p.x + p.w / 2.0, 200.0), Some(Hit::Body));
        assert_eq!(regions.hit(p.x - 5.0, 200.0), None);
    }

    #[test]
    fn the_drawn_shoulders_and_edge_are_the_island_and_the_halo_is_its_own() {
        let n = shape_for(Style::Notch, ShapeKind::Idle);
        let mut regions = HitRegions {
            shape: Some((Style::Notch, n)),
            ..Default::default()
        };
        // The shoulders' filled corner, right against the top edge.
        assert_eq!(regions.hit(n.x - 2.0, n.y + 1.0), Some(Hit::Body));
        assert_eq!(regions.hit(n.x + n.w + 2.0, n.y + 1.0), Some(Hit::Body));
        // Inside the shoulder's curve is not drawn.
        assert_eq!(
            regions.hit(n.x - SHOULDER + 1.0, n.y + SHOULDER - 1.0),
            None
        );
        // The anti-aliased pixel just past the side and the bottom.
        assert_eq!(regions.hit(n.x - 0.5, n.y + 15.0), Some(Hit::Body));
        assert_eq!(regions.hit(n.x - 3.0, n.y + 15.0), None);
        assert_eq!(
            regions.hit(n.x + n.w / 2.0, n.y + n.h + 0.5),
            Some(Hit::Body)
        );
        assert_eq!(regions.hit(n.x + n.w / 2.0, n.y + n.h + 3.0), None);
        // A Pill has no shoulders.
        let p = shape_for(Style::Pill, ShapeKind::Idle);
        regions.shape = Some((Style::Pill, p));
        assert_eq!(regions.hit(p.x - 2.0, p.y + 1.0), None);
        assert_eq!(regions.hit(p.x + p.w / 2.0, p.y - 0.5), Some(Hit::Body));
        // The drag margin is neither the island nor nothing.
        assert_eq!(regions.under(p.x - 6.0, 2.0), Under::Off);
        regions.halo = Some(Rect {
            x: p.x - 14.0,
            y: 0.0,
            w: p.w + 28.0,
            h: p.y + p.h + 8.0,
        });
        assert_eq!(regions.under(p.x - 6.0, 2.0), Under::Halo);
        assert_eq!(regions.hit(p.x - 6.0, 2.0), None);
        assert_eq!(
            regions.under(p.x + p.w / 2.0, p.y + p.h / 2.0),
            Under::Island(Hit::Body)
        );
    }

    #[test]
    fn badge_colors_by_source_and_kind() {
        use crate::theme::{Accent, Surface, island};
        let p = island(Accent::Plum, Surface::Espresso);
        let style = |n: &Notice| badge_style(n, &p, Phase::ShortBreak);
        let fill = |n: &Notice| {
            let b = style(n);
            (b.fill, b.ink, b.icon)
        };
        let app = |s: &str| {
            notice(
                s,
                NoticeKind::App {
                    app_id: None,
                    toast: None,
                },
            )
        };
        assert_eq!(fill(&app("WhatsApp")), (WHATSAPP, WHATSAPP, None));
        assert_eq!(fill(&app("whatsapp beta")), (WHATSAPP, WHATSAPP, None));
        assert_eq!(fill(&app("Discord")), (p.raised_hi, p.text, None));
        assert_eq!(style(&app("Discord")).alpha, 1.0);
        let done = NoticeKind::ClaudeDone {
            session_id: "s".into(),
        };
        // Claude on a tint of the accent, as in the design.
        let claude = (0xB07A9A, 0xB07A9A, Some(Icon::Sparkle));
        assert_eq!(fill(&notice("Claude", done)), claude);
        let waiting = NoticeKind::ClaudeWaiting {
            session_id: "s".into(),
        };
        assert_eq!(fill(&notice("Claude", waiting)), claude);
        let battery = notice("Baterai", NoticeKind::BatteryLow);
        assert_eq!(fill(&battery), (0xD9A441, 0xD9A441, Some(Icon::Battery)));
        assert!(style(&battery).alpha < 0.5, "a tint");
        let system = notice("Sistem", NoticeKind::SystemLow);
        assert_eq!(fill(&system), (0xD9A441, 0xD9A441, Some(Icon::Activity)));
        // A finished clean: sage, the sparkle (the robot's Happy face when on).
        let clean = notice("Bersihkan", NoticeKind::CleanDone);
        assert_eq!(fill(&clean), (0x9DB59A, 0x9DB59A, Some(Icon::Sparkle)));
        assert!(style(&clean).alpha < 0.5, "a tint");
        // Mole: done like a clean, failed like a low disk.
        let mole = notice("Bersihkan", NoticeKind::MoleDone);
        assert_eq!(fill(&mole), (0x9DB59A, 0x9DB59A, Some(Icon::Sparkle)));
        let failed = notice("Bersihkan", NoticeKind::MoleFailed);
        assert_eq!(fill(&failed), (0xD9A441, 0xD9A441, Some(Icon::Activity)));
        // A drive plugged in: the accent's flash drive; safe to remove: sage.
        let plugged = notice("Perangkat", NoticeKind::DevicePlugged { letter: 'E' });
        assert_eq!(fill(&plugged), (p.accent, p.accent, Some(Icon::Usb)));
        let safe = notice("Perangkat", NoticeKind::DeviceSafe);
        assert_eq!(fill(&safe), (0x9DB59A, 0x9DB59A, Some(Icon::Check)));
        assert!(style(&plugged).alpha < 0.5 && style(&safe).alpha < 0.5);
        // Pomodoro: the colour of the phase a click starts.
        let pomo = notice("Pomodoro", NoticeKind::Pomodoro);
        assert_eq!(fill(&pomo), (0x9DB59A, 0x9DB59A, Some(Icon::Timer)));
        let focus = badge_style(&pomo, &p, Phase::Focus);
        assert_eq!((focus.fill, focus.ink), (p.accent, p.accent));
        // Reminders follow the accent, solid, with its ink.
        let reminder = NoticeKind::Reminder { id: "r".into() };
        let r = style(&notice("Pengingat", reminder));
        assert_eq!((r.fill, r.alpha), (0xB07A9A, 1.0));
        assert_eq!((r.ink, r.icon), (p.on_accent, Some(Icon::Bell)));
    }

    #[test]
    fn notice_chip_only_where_a_click_acts() {
        let reminder = NoticeKind::Reminder { id: "r".into() };
        assert_eq!(
            notice_action(&reminder, PomoStatus::Idle),
            Some(Key::MarkDone)
        );
        assert_eq!(
            notice_action(&NoticeKind::Pomodoro, PomoStatus::Ready),
            Some(Key::Start)
        );
        // The next phase was already started elsewhere: a click starts nothing.
        let started = [
            PomoStatus::Idle,
            PomoStatus::Running {
                ends_at: std::time::Instant::now(),
            },
            PomoStatus::Paused {
                remaining: std::time::Duration::from_secs(60),
            },
        ];
        for status in started {
            assert_eq!(notice_action(&NoticeKind::Pomodoro, status), None);
        }
        // A click on a Claude notice only dismisses it.
        let done = NoticeKind::ClaudeDone {
            session_id: "s".into(),
        };
        let waiting = NoticeKind::ClaudeWaiting {
            session_id: "s".into(),
        };
        assert_eq!(notice_action(&done, PomoStatus::Ready), None);
        assert_eq!(notice_action(&waiting, PomoStatus::Ready), None);
        assert_eq!(
            notice_action(&NoticeKind::BatteryLow, PomoStatus::Ready),
            None
        );
        // Disk or RAM low: "Buka" opens the System page.
        assert_eq!(
            notice_action(&NoticeKind::SystemLow, PomoStatus::Idle),
            Some(Key::Open)
        );
        // A plugged-in drive: "Buka" opens Explorer on it; "Aman dicabut"
        // only informs.
        assert_eq!(
            notice_action(&NoticeKind::DevicePlugged { letter: 'E' }, PomoStatus::Idle),
            Some(Key::Open)
        );
        assert_eq!(
            notice_action(&NoticeKind::DeviceSafe, PomoStatus::Idle),
            None
        );
        // A finished clean: a click only dismisses it.
        assert_eq!(
            notice_action(&NoticeKind::CleanDone, PomoStatus::Idle),
            None
        );
        // Mole's notices open its log.
        for kind in [NoticeKind::MoleDone, NoticeKind::MoleFailed] {
            assert_eq!(notice_action(&kind, PomoStatus::Idle), Some(Key::MoleLog));
        }
        // A copy preview: "Riwayat" opens the Clipboard page; "Disalin" has
        // no chip.
        assert_eq!(
            notice_action(&NoticeKind::ClipPreview { id: 1 }, PomoStatus::Idle),
            Some(Key::ClipPreviewChip)
        );
        assert_eq!(notice_action(&NoticeKind::Copied, PomoStatus::Idle), None);
        assert_eq!(
            notice_action_w(&NoticeKind::ClipPreview { id: 1 }),
            ACTION_W
        );
    }

    #[test]
    fn a_sticky_note_reminder_has_two_chips_and_the_reminders_badge() {
        use crate::theme::{Accent, Surface, island};
        let p = island(Accent::Plum, Surface::Espresso);
        let kind = NoticeKind::StickyReminder { id: "n1".into() };
        assert_eq!(notice_action(&kind, PomoStatus::Idle), None, "two chips of its own");
        let widths = notice_chip_widths(&kind, PomoStatus::Idle);
        assert_eq!(widths, vec![SNOOZE_CHIP_W, UPDATE_CHIP_W]);
        for style in [Style::Pill, Style::Notch] {
            for robot in [false, true] {
                let n = shape_with_robot_sized(style, NotchSize::DEFAULT, ShapeKind::Notice, robot);
                let (l, chips) = notice_actions_layout(n, &widths);
                let [snooze, open] = chips[..] else {
                    panic!("two chips");
                };
                let tag = format!("{style:?} robot {robot}");
                for chip in [snooze, open] {
                    assert!(inside(n, chip), "{tag} {chip:?}");
                    for text in [l.title, l.body, l.badge] {
                        assert!(!intersects(chip, text), "{tag} {chip:?} {text:?}");
                    }
                }
                assert!(snooze.right() + NOTICE_CHIP_GAP <= open.x + 0.001, "{tag}");
                assert!(l.title.right() < snooze.x, "{tag}: the text stops short");
                // Both chips are click targets; the rest is the body.
                let regions = HitRegions {
                    shape: Some((style, n)),
                    regions: sticky_reminder_regions(snooze, open),
                    ..HitRegions::default()
                };
                let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
                let (x, y) = mid(snooze);
                assert_eq!(regions.hit(x, y), Some(Hit::StickySnooze), "{tag}");
                let (x, y) = mid(open);
                assert_eq!(regions.hit(x, y), Some(Hit::StickyOpen), "{tag}");
                assert_eq!(regions.hit(n.x + 30.0, n.y + n.h / 2.0), Some(Hit::Body), "{tag}");
            }
        }
        // The same bell badge as a reminder.
        let notice = Notice {
            key: "k".into(),
            source: "Vale".into(),
            title: "t".into(),
            body: "b".into(),
            kind,
        };
        let badge = badge_style(&notice, &p, Phase::Focus);
        assert_eq!((badge.fill, badge.alpha, badge.icon), (p.accent, 1.0, Some(Icon::Bell)));
    }

    fn rec_saved(offer: TranscriptOffer) -> NoticeKind {
        NoticeKind::RecSaved {
            path: "a.m4a".into(),
            offer,
            thumb: None,
        }
    }

    #[test]
    fn rec_saved_chip_widths_follow_the_offer() {
        assert_eq!((TRANSCRIBE_CHIP_W, SETUP_CHIP_W), (76.0, 84.0));
        let idle = PomoStatus::Idle;
        assert_eq!(
            notice_chip_widths(&rec_saved(TranscriptOffer::Transcribe), idle),
            vec![TRANSCRIBE_CHIP_W]
        );
        assert_eq!(
            notice_chip_widths(&rec_saved(TranscriptOffer::Setup), idle),
            vec![SETUP_CHIP_W]
        );
        assert!(notice_chip_widths(&rec_saved(TranscriptOffer::None), idle).is_empty());
        assert_eq!(
            notice_action(&rec_saved(TranscriptOffer::Transcribe), idle),
            Some(Key::TrChipTranscribe)
        );
        assert_eq!(
            notice_action(&rec_saved(TranscriptOffer::Setup), idle),
            Some(Key::TrChipSettings)
        );
        assert_eq!(notice_action(&rec_saved(TranscriptOffer::None), idle), None);
        // The transcript notices have no chip: their body is the click.
        for kind in [
            NoticeKind::TranscriptReady {
                audio: "a.m4a".into(),
            },
            NoticeKind::TranscriptFailed {
                audio: "a.m4a".into(),
            },
            NoticeKind::TranscriptInfo,
            NoticeKind::TranscriptStarting {
                audio: r"C:\r\a.m4a".into(),
            },
            NoticeKind::TranscriptSettings,
        ] {
            assert!(notice_chip_widths(&kind, idle).is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn rec_saved_regions_hit_their_chip() {
        for (offer, hit) in [
            (TranscriptOffer::Transcribe, Hit::RecTranscribeChip),
            (TranscriptOffer::Setup, Hit::RecTranscriptSetup),
        ] {
            let kind = rec_saved(offer);
            let widths = notice_chip_widths(&kind, PomoStatus::Idle);
            for style in [Style::Pill, Style::Notch] {
                for robot in [false, true] {
                    let n =
                        shape_with_robot_sized(style, NotchSize::DEFAULT, ShapeKind::Notice, robot);
                    // A saved notice is closable: the layout keeps the ✕'s room.
                    let l = notice_layout_with(n, Some(widths[0]), true);
                    let chip = l.action.expect("a chip");
                    let tag = format!("{offer:?} {style:?} robot {robot}");
                    assert_eq!(chip.w, widths[0], "{tag}");
                    assert!(inside(n, chip), "{tag} {chip:?}");
                    for text in [l.title, l.body, l.badge] {
                        assert!(!intersects(chip, text), "{tag} {chip:?} {text:?}");
                    }
                    let regions = HitRegions {
                        shape: Some((style, n)),
                        regions: rec_saved_regions(chip, offer),
                        ..HitRegions::default()
                    };
                    let (x, y) = (chip.x + chip.w / 2.0, chip.y + chip.h / 2.0);
                    assert_eq!(regions.hit(x, y), Some(hit.clone()), "{tag}");
                    assert_eq!(regions.hit(n.x + 30.0, n.y + n.h / 2.0), Some(Hit::Body), "{tag}");
                }
            }
        }
        // No offer, no chip, nothing to hit.
        let chip = Rect {
            x: 10.0,
            y: 10.0,
            w: TRANSCRIBE_CHIP_W,
            h: 24.0,
        };
        assert!(rec_saved_regions(chip, TranscriptOffer::None).is_empty());
    }

    #[test]
    fn the_transcript_notices_wear_the_notes_icon_in_their_own_tint() {
        use crate::theme::{Accent, Surface, island};
        let p = island(Accent::Plum, Surface::Espresso);
        let badge = |kind: NoticeKind| {
            let n = Notice {
                key: "k".into(),
                source: "Transkrip".into(),
                title: "t".into(),
                body: "b".into(),
                kind,
            };
            badge_style(&n, &p, Phase::Focus)
        };
        let ready = badge(NoticeKind::TranscriptReady {
            audio: "a.m4a".into(),
        });
        assert_eq!((ready.fill, ready.icon), (p.sage, Some(Icon::Note)));
        let failed = badge(NoticeKind::TranscriptFailed {
            audio: "a.m4a".into(),
        });
        assert_eq!(
            (failed.fill, failed.icon),
            (crate::theme::REC_RED, Some(Icon::Note))
        );
        for kind in [
            NoticeKind::TranscriptInfo,
            NoticeKind::TranscriptStarting {
                audio: r"C:\r\a.m4a".into(),
            },
            NoticeKind::TranscriptSettings,
        ] {
            let b = badge(kind);
            assert_eq!((b.fill, b.icon), (p.accent, Some(Icon::Note)));
        }
    }

    #[test]
    fn the_update_notices_have_chips_and_badges() {
        use crate::theme::{Accent, Surface, island};
        let p = island(Accent::Plum, Surface::Espresso);
        let available = NoticeKind::UpdateAvailable {
            version: "3.11.0".into(),
        };
        // The offer has two chips of its own, not one action.
        assert_eq!(notice_action(&available, PomoStatus::Idle), None);
        assert_eq!(
            notice_chip_widths(&available, PomoStatus::Idle),
            vec![LATER_CHIP_W, UPDATE_CHIP_W]
        );
        assert_eq!(
            notice_chip_widths(&NoticeKind::UpdateFailed, PomoStatus::Idle),
            vec![RELEASES_ACTION_W]
        );
        assert!(notice_chip_widths(&NoticeKind::UpdateInfo, PomoStatus::Idle).is_empty());
        assert_eq!(
            notice_action(&NoticeKind::UpdateFailed, PomoStatus::Idle),
            Some(Key::SeeReleases)
        );
        for kind in [NoticeKind::UpdateProgress { pct: 5 }, NoticeKind::UpdateInfo] {
            assert_eq!(notice_action(&kind, PomoStatus::Idle), None);
        }
        assert_eq!(
            notice_action_w(&NoticeKind::UpdateFailed),
            RELEASES_ACTION_W
        );
        let icon = |kind: NoticeKind| badge_style(&notice("Vale", kind), &p, Phase::Focus).icon;
        assert_eq!(icon(available), Some(Icon::Download));
        assert_eq!(
            icon(NoticeKind::UpdateProgress { pct: 5 }),
            Some(Icon::Download)
        );
        assert_eq!(icon(NoticeKind::UpdateFailed), Some(Icon::Download));
        assert_eq!(icon(NoticeKind::UpdateInfo), Some(Icon::Check));
        // Ochre for the failure, the accent for the offer.
        let fill = |kind: NoticeKind| badge_style(&notice("Vale", kind), &p, Phase::Focus).fill;
        assert_eq!(fill(NoticeKind::UpdateFailed), p.ochre);
        assert_eq!(fill(NoticeKind::UpdateProgress { pct: 5 }), p.accent);
        assert_eq!(fill(NoticeKind::UpdateInfo), p.sage);
    }

    #[test]
    fn badge_letter_is_first_char_uppercased() {
        assert_eq!(badge_letter("whatsApp"), "W");
        assert_eq!(badge_letter("ärzte"), "Ä");
        assert_eq!(badge_letter(""), "");
    }

    #[test]
    fn pill_compact_is_centered_below_top() {
        assert_eq!(
            shape_for(Style::Pill, ShapeKind::MediaCompact),
            ShapeRect {
                x: 209.0,
                y: 8.0,
                w: 202.0,
                h: 34.0,
                radius: 17.0
            }
        );
    }

    #[test]
    fn notch_is_flush_with_top() {
        assert_eq!(shape_for(Style::Notch, ShapeKind::Notice).y, 0.0);
    }

    #[test]
    fn sizes_match_plan() {
        for style in [Style::Pill, Style::Notch] {
            let size = |kind| {
                let s = shape_for(style, kind);
                assert_eq!(s.x * 2.0 + s.w, WINDOW_W, "{style:?} {s:?} not centered");
                (s.w, s.h)
            };
            assert_eq!(size(ShapeKind::Idle), (190.0, 34.0));
            assert_eq!(size(ShapeKind::MediaCompact), (202.0, 34.0));
            assert_eq!(size(ShapeKind::PomodoroCompact), (212.0, 34.0));
            assert_eq!(size(ShapeKind::MediaLyric), (220.0, 34.0));
            assert_eq!(size(ShapeKind::Notice), (360.0, 76.0));
            assert_eq!(size(ShapeKind::Panel), (560.0, 290.0));
        }
    }

    #[test]
    fn radii_match_plan() {
        let radius = |style, kind| shape_for(style, kind).radius;
        assert_eq!(radius(Style::Pill, ShapeKind::Notice), 26.0);
        assert_eq!(radius(Style::Pill, ShapeKind::Panel), 30.0);
        assert_eq!(radius(Style::Pill, ShapeKind::PomodoroCompact), 17.0);
        assert_eq!(radius(Style::Notch, ShapeKind::MediaCompact), 16.0);
        assert_eq!(radius(Style::Notch, ShapeKind::PomodoroCompact), 16.0);
        assert_eq!(radius(Style::Pill, ShapeKind::MediaLyric), 17.0);
        assert_eq!(radius(Style::Notch, ShapeKind::MediaLyric), 16.0);
        assert_eq!(radius(Style::Notch, ShapeKind::Notice), 26.0);
        assert_eq!(radius(Style::Notch, ShapeKind::Panel), 30.0);
        // Drawn as stored: no clamping at rest.
        for style in [Style::Pill, Style::Notch] {
            for kind in [ShapeKind::Notice, ShapeKind::Panel] {
                let s = shape_for(style, kind);
                assert_eq!(corner_radius(style, s), s.radius, "{style:?} {kind:?}");
            }
        }
    }

    #[test]
    fn equalizer_follows_formula_and_rests_when_paused() {
        let at0 = equalizer_heights(0.0, true);
        let expect = |phase: f32| BAR_MAX_H * (0.35 + 0.65 * phase.sin().abs());
        assert_eq!(at0.len(), 5);
        for (h, phase) in at0.iter().zip([0.0, 1.1, 2.3, 0.6, 1.7]) {
            assert!((h - expect(phase)).abs() < 1e-4);
        }
        // The render clock wraps at 10 s: every bar is back where it started.
        for t in [0.0, 0.37, 2.5, 7.9] {
            let (a, b) = (
                equalizer_heights(t, true),
                equalizer_heights(t + 10.0, true),
            );
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() < 1e-3, "t={t}: {x} vs {y}");
            }
        }
        // Bar 0 (1.7 Hz) peaks a quarter period in.
        let peak = equalizer_heights(1.0 / (4.0 * 1.7), true)[0];
        assert!((peak - BAR_MAX_H).abs() < 1e-3);
        for i in 0..200 {
            for h in equalizer_heights(i as f32 * 0.033, true) {
                assert!((0.35 * BAR_MAX_H - 1e-4..=BAR_MAX_H + 1e-4).contains(&h));
            }
        }
        let paused = equalizer_heights(12.3, false);
        for h in paused {
            assert!((h - BAR_MAX_H * 0.675).abs() < 1e-4);
        }
        assert_eq!(paused, equalizer_heights(0.0, false), "static");
    }

    #[test]
    fn audio_bars_rest_at_a_fifth_and_rise_with_the_level() {
        let rest = BAR_MAX_H * EQ_REST;
        assert_eq!(
            audio_bar_heights([0.0; BAR_COUNT], false),
            [rest; BAR_COUNT]
        );
        assert_eq!(
            audio_bar_heights([1.0; BAR_COUNT], false),
            [BAR_MAX_H; BAR_COUNT]
        );
        let mid = audio_bar_heights([0.5; BAR_COUNT], false)[0];
        assert!((mid - BAR_MAX_H * 0.6).abs() < 1e-4);
        // Heights follow the levels bar by bar and never fall as a level rises.
        let h = audio_bar_heights([0.0, 0.25, 0.5, 0.75, 1.0], false);
        assert!(h.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn audio_bars_stay_in_range_and_survive_bad_levels() {
        let (lo, hi) = (BAR_MAX_H * EQ_REST, BAR_MAX_H);
        let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -3.0, 7.5];
        for h in audio_bar_heights(bad, false) {
            assert!((lo..=hi).contains(&h), "{h}");
        }
        assert_eq!(audio_bar_heights(bad, false)[0], lo, "NaN is silence");
    }

    #[test]
    fn muted_audio_bars_all_rest() {
        let rest = BAR_MAX_H * EQ_REST;
        assert_eq!(
            audio_bar_heights([1.0, 0.7, 0.3, 1.0, 0.5], true),
            [rest; BAR_COUNT]
        );
    }

    #[test]
    fn ease_endpoints_and_overshoot() {
        assert!(ease_out_back(0.0).abs() < 1e-6);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-6);
        let peak = (0..=100)
            .map(|i| ease_out_back(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.0 && peak < 1.1);
    }

    #[test]
    fn lerp_halfway() {
        let a = shape_for(Style::Pill, ShapeKind::Idle);
        let b = shape_for(Style::Pill, ShapeKind::Panel);
        let m = lerp_shape(a, b, 0.5);
        assert_eq!(m.w, (a.w + b.w) / 2.0);
        assert_eq!(m.x * 2.0 + m.w, WINDOW_W);
        assert_eq!(lerp_shape(a, b, 0.0), a);
        assert_eq!(lerp_shape(a, b, 1.0), b);
    }

    #[test]
    fn lyric_width_stays_between_min_and_max() {
        for robot in [false, true] {
            // With the robot the width is the clamped one plus its extra lead.
            let x = if robot { LYRIC_ROBOT_EXTRA } else { 0.0 };
            // Instrumental "♪", nothing, or a failed measure: the minimum.
            assert_eq!(lyric_width(None, robot, LYRIC_MAX_W), LYRIC_MIN_W + x);
            assert_eq!(lyric_width(Some(0.0), robot, LYRIC_MAX_W), LYRIC_MIN_W + x);
            // A short line keeps the minimum; a huge one caps at the maximum.
            assert_eq!(lyric_width(Some(40.0), robot, LYRIC_MAX_W), LYRIC_MIN_W + x);
            assert_eq!(
                lyric_width(Some(2000.0), robot, LYRIC_MAX_W),
                LYRIC_MAX_W + x
            );
            assert_eq!(lyric_width(Some(2000.0), robot, 450.0), 450.0 + x);
            // In between, the notch hugs its line: text, 1 px slack, lead
            // (the robot's, before its inset grew: 40), padding.
            let lead = if robot { 40.0 } else { 20.0 };
            assert_eq!(
                lyric_width(Some(300.4), robot, LYRIC_MAX_W),
                301.0 + 1.0 + lead + 20.0 + x
            );
            for i in 0..200 {
                let w = lyric_width(Some(i as f32 * 4.0), robot, LYRIC_MAX_W);
                assert!((LYRIC_MIN_W + x..=LYRIC_MAX_W + x).contains(&w), "{w}");
            }
        }
        // A maximum below the minimum (a tiny screen) never undercuts it.
        assert_eq!(lyric_width(Some(500.0), false, 100.0), LYRIC_MIN_W);
    }

    #[test]
    fn lyric_max_follows_the_screen() {
        assert_eq!(lyric_max_w(1000.0), 450.0);
        // Wide screens cap at 560; tiny ones never go below 220.
        assert_eq!(lyric_max_w(1920.0), LYRIC_MAX_W);
        assert_eq!(lyric_max_w(3840.0), LYRIC_MAX_W);
        assert_eq!(lyric_max_w(400.0), LYRIC_MIN_W);
        assert_eq!(lyric_max_w(0.0), LYRIC_MIN_W);
        // 1366 × 0.45 = 614.7: still 560.
        assert_eq!(lyric_max_w(1366.0), LYRIC_MAX_W);
        assert!((lyric_max_w(1100.0) - 495.0).abs() < 1e-3);
    }

    #[test]
    fn lyric_width_shrinks_only_by_the_slack() {
        // Just opened: whatever the line wants.
        assert_eq!(lyric_target_w(None, 300.0), 300.0);
        assert_eq!(lyric_target_w(None, LYRIC_MIN_W), LYRIC_MIN_W);
        // Longer: grows at once, by any amount.
        assert_eq!(lyric_target_w(Some(300.0), 301.0), 301.0);
        assert_eq!(lyric_target_w(Some(300.0), 560.0), 560.0);
        // Up to 40 px shorter keeps the width; 40 or more shrinks.
        assert_eq!(lyric_target_w(Some(300.0), 300.0), 300.0);
        assert_eq!(lyric_target_w(Some(300.0), 265.0), 300.0);
        assert_eq!(lyric_target_w(Some(300.0), 260.5), 300.0);
        assert_eq!(lyric_target_w(Some(300.0), 260.0), 260.0);
        assert_eq!(lyric_target_w(Some(300.0), 220.0), 220.0);
    }

    #[test]
    fn lyric_shape_is_a_centred_compact_shape() {
        for style in [Style::Pill, Style::Notch] {
            let base = shape_for(style, ShapeKind::MediaLyric);
            for w in [LYRIC_MIN_W, 333.0, LYRIC_MAX_W] {
                let s = lyric_shape(style, w);
                assert_eq!(s.w, w);
                assert_eq!(s.x * 2.0 + s.w, WINDOW_W);
                assert_eq!((s.y, s.h, s.radius), (base.y, base.h, base.radius));
                assert_eq!(s.h, 34.0);
            }
            // The widest notch, overshooting, still fits the window.
            let peak = (0..=100)
                .map(|i| ease_out_back(i as f32 / 100.0))
                .fold(0.0, f32::max);
            let grown = lerp_shape(
                lyric_shape(style, LYRIC_MIN_W),
                lyric_shape(style, LYRIC_MAX_W),
                peak,
            );
            assert!(grown.x >= SHOULDER && grown.x + grown.w <= WINDOW_W - SHOULDER);
        }
    }

    #[test]
    fn a_line_that_fits_the_widest_box_is_never_trimmed() {
        for style in [Style::Pill, Style::Notch] {
            for robot in [false, true] {
                let x = if robot { LYRIC_ROBOT_EXTRA } else { 0.0 };
                let room = lyric_box_w(LYRIC_MAX_W + x, robot) - 1.0;
                for i in 0..=100 {
                    let text_w = room * i as f32 / 100.0 + 0.37;
                    let s = lyric_shape(style, lyric_width(Some(text_w), robot, LYRIC_MAX_W));
                    let area = if robot {
                        lyric_layout_robot(s).1
                    } else {
                        lyric_layout(s)
                    };
                    assert!(area.w >= text_w, "{style:?} {robot} {text_w} in {area:?}");
                    assert!((area.w - lyric_box_w(s.w, robot)).abs() < 1e-3);
                    assert!(!lyric_overflow(text_w, area.w));
                    assert_eq!(lyric_scroll_end_ms(text_w, area.w), None);
                }
            }
        }
    }

    #[test]
    fn a_long_line_holds_then_scrolls_to_its_end() {
        let (text_w, box_w) = (700.0, 500.0);
        assert!(lyric_overflow(text_w, box_w));
        // Held still for the first second.
        for ms in [0, 500, LYRIC_HOLD_MS] {
            assert_eq!(lyric_scroll_offset(text_w, box_w, ms), 0.0);
        }
        // Then 30 px/s.
        let after = lyric_scroll_offset(text_w, box_w, LYRIC_HOLD_MS + 1000);
        assert!((after - LYRIC_SCROLL_PX_PER_S).abs() < 1e-3);
        // Rising, never past the end, then resting there.
        let end = lyric_scroll_end_ms(text_w, box_w).expect("scrolls");
        let max = text_w - box_w + LYRIC_FADE_W;
        let mut last = 0.0;
        for ms in (0..end + 3000).step_by(37) {
            let o = lyric_scroll_offset(text_w, box_w, ms);
            assert!(o >= last && o <= max, "{ms}: {o}");
            last = o;
        }
        assert_eq!(lyric_scroll_offset(text_w, box_w, end), max);
        assert_eq!(lyric_scroll_offset(text_w, box_w, end + 60_000), max);
        assert!(lyric_scroll_offset(text_w, box_w, end - 100) < max);
        // At its end the line's last letter is left of the fade.
        assert!(box_w - (text_w - max) >= LYRIC_FADE_W - 1e-3);
        // A line that fits never moves.
        assert_eq!(lyric_scroll_offset(400.0, box_w, 10_000), 0.0);
        assert_eq!(lyric_scroll_end_ms(400.0, box_w), None);
        assert_eq!(lyric_scroll_end_ms(box_w, box_w), None);
    }

    const ALL_KINDS: [ShapeKind; 10] = [
        ShapeKind::Idle,
        ShapeKind::MediaCompact,
        ShapeKind::MediaLyric,
        ShapeKind::PomodoroCompact,
        ShapeKind::RecordCompact,
        ShapeKind::Notice,
        ShapeKind::NoticeShot,
        ShapeKind::Panel,
        ShapeKind::PanelTall,
        ShapeKind::PanelClean,
    ];

    fn sizes() -> [NotchSize; 3] {
        [
            NotchSize::from_px(NOTCH_W_MIN, NOTCH_H_MIN),
            NotchSize::from_px(NOTCH_W_MAX, NOTCH_H_MAX),
            NotchSize::from_px(NOTCH_W_MIN, NOTCH_H_MAX),
        ]
    }

    #[test]
    fn the_default_size_gives_todays_shapes() {
        assert_eq!(NotchSize::DEFAULT, NotchSize { w: 190.0, h: 34.0 });
        assert_eq!(
            NotchSize::from_config(&crate::config::Config::default()),
            NotchSize::DEFAULT
        );
        for style in [Style::Pill, Style::Notch] {
            for kind in ALL_KINDS {
                let s = shape_sized(style, NotchSize::DEFAULT, kind);
                assert_eq!(s, shape_for(style, kind));
                for robot in [false, true] {
                    assert_eq!(
                        shape_with_robot_sized(style, NotchSize::DEFAULT, kind, robot),
                        shape_with_robot(style, kind, robot)
                    );
                }
            }
            assert_eq!(
                lyric_shape_sized(style, NotchSize::DEFAULT, 300.0),
                lyric_shape(style, 300.0)
            );
        }
        // Today's numbers, spelled out.
        let idle = shape_for(Style::Pill, ShapeKind::Idle);
        assert_eq!((idle.w, idle.h), (190.0, 34.0));
        assert_eq!(shape_for(Style::Pill, ShapeKind::PomodoroCompact).w, 212.0);
        assert_eq!(shape_for(Style::Pill, ShapeKind::RecordCompact).w, 214.0);
        assert_eq!(shape_for(Style::Pill, ShapeKind::MediaCompact).w, 202.0);
        assert_eq!(shape_for(Style::Pill, ShapeKind::MediaLyric).w, LYRIC_MIN_W);
    }

    #[test]
    fn a_user_size_changes_only_the_compact_shapes() {
        for style in [Style::Pill, Style::Notch] {
            for size in sizes() {
                for kind in ALL_KINDS {
                    let s = shape_sized(style, size, kind);
                    let base = shape_for(style, kind);
                    match kind {
                        ShapeKind::Idle => {
                            assert_eq!((s.w, s.h), (size.w, size.h));
                        }
                        ShapeKind::MediaCompact => {
                            assert_eq!((s.w, s.h), (size.w + 12.0, size.h));
                        }
                        ShapeKind::PomodoroCompact => {
                            assert_eq!((s.w, s.h), (size.w + 22.0, size.h));
                        }
                        ShapeKind::RecordCompact => {
                            assert_eq!((s.w, s.h), (size.w + 24.0, size.h));
                        }
                        ShapeKind::MediaLyric => {
                            assert_eq!((s.w, s.h), (LYRIC_MIN_W.max(size.w), size.h));
                        }
                        _ => assert_eq!(s, base, "{kind:?} must not follow the size"),
                    }
                    // Always centred, whatever the width.
                    assert!((s.x + s.w / 2.0 - WINDOW_W / 2.0).abs() < 1e-3);
                }
                let idle = shape_sized(style, size, ShapeKind::Idle);
                match style {
                    Style::Pill => assert_eq!(idle.radius, size.h / 2.0),
                    Style::Notch => assert_eq!(idle.radius, NOTCH_COMPACT_RADIUS),
                }
                // The Notch keeps its full 16 px corner at the minimum.
                assert!(corner_radius(style, idle) >= 14.0, "{size:?}");
            }
        }
    }

    #[test]
    fn size_from_px_is_clamped() {
        assert_eq!(NotchSize::from_px(0, 0), NotchSize { w: 160.0, h: 28.0 });
        assert_eq!(
            NotchSize::from_px(9999, 9999),
            NotchSize { w: 300.0, h: 44.0 }
        );
    }

    #[test]
    fn the_lyric_minimum_follows_the_width() {
        assert_eq!(lyric_width_min(None, false, 160.0, 560.0), LYRIC_MIN_W);
        assert_eq!(lyric_width_min(None, false, 300.0, 560.0), 300.0);
        // With the robot: the same plus the robot's extra lead (10).
        assert_eq!(lyric_width_min(Some(10.0), true, 300.0, 560.0), 310.0);
        // A long line still grows past it; the cap never sits below it.
        assert_eq!(lyric_width_min(Some(400.0), false, 300.0, 560.0), 441.0);
        assert_eq!(lyric_width_min(Some(400.0), false, 300.0, 250.0), 300.0);
        // The default minimum is `lyric_width`.
        for t in [None, Some(50.0), Some(300.0), Some(900.0)] {
            assert_eq!(
                lyric_width_min(t, true, LYRIC_MIN_W, LYRIC_MAX_W),
                lyric_width(t, true, LYRIC_MAX_W)
            );
        }
    }

    #[test]
    fn content_stays_inside_at_the_smallest_and_largest_size() {
        for style in [Style::Pill, Style::Notch] {
            for size in sizes() {
                let head_inside = |s: ShapeRect, robot: Rect| {
                    for (x, y) in head_outline(robot) {
                        assert!(
                            shape_contains(style, s, x, y),
                            "{style:?} {size:?} {robot:?} ({x}, {y})"
                        );
                    }
                };
                for (robot, clock) in [(true, true), (true, false), (false, true)] {
                    let s = shape_with_robot_sized(style, size, ShapeKind::Idle, robot);
                    let l = idle_layout(s, robot, clock);
                    if let Some(r) = l.robot {
                        head_inside(s, r);
                    }
                    if let Some(c) = l.clock {
                        assert!(inside(s, c), "{style:?} {size:?} {c:?}");
                    }
                    if let (Some(r), Some(c)) = (l.robot, l.clock) {
                        assert!(!intersects(r, c));
                    }
                }
                // Compact media, with and without the robot.
                let s = shape_with_robot_sized(style, size, ShapeKind::MediaCompact, false);
                let l = compact_layout(s);
                for r in [l.art, l.text, l.bars] {
                    assert!(inside(s, r), "{style:?} {size:?} media {r:?}");
                }
                assert!(l.text.w > 40.0, "{size:?} the title keeps some room");
                let s = shape_with_robot_sized(style, size, ShapeKind::MediaCompact, true);
                let l = compact_layout_robot(s);
                head_inside(s, l.art);
                for r in [l.text, l.bars] {
                    assert!(inside(s, r) && !intersects(l.art, r), "{size:?} {r:?}");
                }
                // The lyric view, both ways.
                for robot in [false, true] {
                    for text_w in [None, Some(120.0), Some(400.0)] {
                        let s = lyric_shape_sized(
                            style,
                            size,
                            lyric_width_min(text_w, robot, size.w, LYRIC_MAX_W),
                        );
                        assert!(s.w >= size.w && s.w >= LYRIC_MIN_W);
                        if robot {
                            let (r, t) = lyric_layout_robot(s);
                            head_inside(s, r);
                            assert!(inside(s, t) && !intersects(r, t));
                        } else {
                            assert!(inside(s, lyric_layout(s)));
                        }
                    }
                }
                // The timer.
                let s = shape_with_robot_sized(style, size, ShapeKind::PomodoroCompact, true);
                let (robot, (cx, cy), r, text) =
                    super::super::panel_layout::pomodoro_compact_layout_robot(s);
                head_inside(s, robot);
                let ring = Rect {
                    x: cx - r - 1.5,
                    y: cy - r - 1.5,
                    w: 2.0 * r + 3.0,
                    h: 2.0 * r + 3.0,
                };
                for part in [ring, text] {
                    assert!(inside(s, part) && !intersects(robot, part), "{part:?}");
                }
                let s = shape_with_robot_sized(style, size, ShapeKind::PomodoroCompact, false);
                let (_, _, text) = super::super::panel_layout::pomodoro_compact_layout(s);
                assert!(inside(s, text));
            }
        }
    }

    /// Sizes spanning the slider ranges, corners included.
    fn spread() -> Vec<NotchSize> {
        [(160, 28), (190, 34), (300, 44), (160, 44), (300, 28)]
            .into_iter()
            .map(|(w, h)| NotchSize::from_px(w, h))
            .collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn bars_width() -> f32 {
        BAR_COUNT as f32 * BAR_W + (BAR_COUNT - 1) as f32 * BAR_GAP
    }

    #[test]
    fn compact_inset_is_the_notch_corner_radius() {
        assert_eq!(COMPACT_INSET, 16.0);
        assert_eq!(COMPACT_INSET, NOTCH_COMPACT_RADIUS);
        assert_eq!(robot_extra(ShapeKind::MediaCompact), 2.0);
        assert_eq!(robot_extra(ShapeKind::PomodoroCompact), 34.0);
        assert_eq!(robot_extra(ShapeKind::RecordCompact), 34.0);
    }

    #[test]
    fn media_compact_sits_the_inset_from_both_ends() {
        for style in [Style::Pill, Style::Notch] {
            for size in spread() {
                for robot in [false, true] {
                    let s = shape_with_robot_sized(style, size, ShapeKind::MediaCompact, robot);
                    let l = if robot {
                        compact_layout_robot(s)
                    } else {
                        compact_layout(s)
                    };
                    let tag = format!("{style:?} {size:?} robot {robot}");
                    assert!(close(l.art.x, s.x + COMPACT_INSET), "{tag} {l:?}");
                    assert!(close(l.bars.right(), s.x + s.w - COMPACT_INSET), "{tag}");
                    assert!(l.art.x >= s.x + 16.0 && l.bars.right() <= s.x + s.w - 16.0);
                    assert!(l.text.x >= l.art.right() && l.text.right() <= l.bars.x);
                }
            }
        }
    }

    #[test]
    fn media_compact_keeps_the_title_room_it_had() {
        // Today's layout from the old constants: art 22 at 6, bars 14 from
        // the right, 10 between; the robot 26 at 6, then 8; the shape the
        // user's width (+2 with the robot).
        for style in [Style::Pill, Style::Notch] {
            for size in spread() {
                let old_plain = size.w - 6.0 - 22.0 - 10.0 - 10.0 - bars_width() - 14.0;
                let old_robot = (size.w + 2.0) - 6.0 - 26.0 - 8.0 - 10.0 - bars_width() - 14.0;
                let plain = shape_with_robot_sized(style, size, ShapeKind::MediaCompact, false);
                let robot = shape_with_robot_sized(style, size, ShapeKind::MediaCompact, true);
                assert!(close(compact_layout(plain).text.w, old_plain), "{size:?}");
                assert!(
                    close(compact_layout_robot(robot).text.w, old_robot),
                    "{size:?}"
                );
                assert!(close(plain.w, size.w + MEDIA_COMPACT_EXTRA));
                assert!(close(robot.w, size.w + MEDIA_COMPACT_EXTRA + 2.0));
            }
        }
    }

    #[test]
    fn idle_keeps_the_users_width() {
        for style in [Style::Pill, Style::Notch] {
            for size in spread() {
                assert_eq!(shape_sized(style, size, ShapeKind::Idle).w, size.w);
                // Robot or not, Idle holds its group inside the same width.
                assert_eq!(
                    shape_with_robot_sized(style, size, ShapeKind::Idle, true).w,
                    size.w
                );
            }
        }
    }

    #[test]
    fn lyric_robot_sits_the_inset_from_the_left() {
        for style in [Style::Pill, Style::Notch] {
            for size in spread() {
                for robot in [false, true] {
                    for text_w in [None, Some(60.0), Some(300.0), Some(900.0)] {
                        let w = lyric_width_min(text_w, robot, size.w, LYRIC_MAX_W);
                        let s = lyric_shape_sized(style, size, w);
                        let line = if robot {
                            let (r, t) = lyric_layout_robot(s);
                            assert!(close(r.x, s.x + COMPACT_INSET), "{style:?} {size:?}");
                            assert!(r.x >= s.x + 16.0);
                            t
                        } else {
                            lyric_layout(s)
                        };
                        assert!(line.x >= s.x + 16.0);
                        assert!(
                            line.right() <= s.x + s.w - 16.0,
                            "{size:?} {robot} {line:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn lyric_cap_is_the_widest_lyric_width() {
        for robot in [false, true] {
            for min_w in [0.0, 220.0, 300.0] {
                for max_w in [220.0, 250.0, 400.0, 560.0] {
                    let cap = lyric_cap(robot, min_w, max_w);
                    for step in 0..=90 {
                        let w = lyric_width_min(Some(step as f32 * 10.0), robot, min_w, max_w);
                        assert!(
                            w <= cap,
                            "robot {robot} min {min_w} max {max_w}: {w} > {cap}"
                        );
                    }
                    assert_eq!(lyric_width_min(Some(2000.0), robot, min_w, max_w), cap);
                }
            }
        }
    }

    #[test]
    fn lyric_box_is_todays_in_every_case() {
        // Today's formula, from the old constants: the lead was 20, or
        // 6 + 26 + 8 = 40 with the robot; right padding 20.
        let old_width = |text_w: f32, robot: bool, min_w: f32, max_w: f32| {
            let lead = if robot { 40.0 } else { 20.0 };
            let min_w = min_w.max(LYRIC_MIN_W);
            let max_w = max_w.max(min_w);
            if text_w > 0.0 {
                (text_w.ceil() + 1.0 + lead + 20.0).clamp(min_w, max_w)
            } else {
                min_w
            }
        };
        for robot in [false, true] {
            let lead = if robot { 40.0 } else { 20.0 };
            for min_w in [220.0, 300.0] {
                for max_w in [220.0, 400.0, 560.0] {
                    for step in 0..=70 {
                        let whole = step as f32 * 10.0;
                        for text_w in [whole, whole + 0.4] {
                            let t = Some(text_w);
                            let now = lyric_box_w(lyric_width_min(t, robot, min_w, max_w), robot);
                            let old =
                                (old_width(text_w, robot, min_w, max_w) - lead - 20.0).max(0.0);
                            assert!(
                                close(now, old),
                                "{t:?} robot {robot} min {min_w} max {max_w}: {now} vs {old}"
                            );
                        }
                    }
                    let now = lyric_box_w(lyric_width_min(None, robot, min_w, max_w), robot);
                    let old = old_width(0.0, robot, min_w, max_w) - lead - 20.0;
                    assert!(close(now, old));
                }
            }
        }
    }

    // ---- the toast slot ----

    /// Every combination the slot can meet: both styles, the smallest,
    /// default and widest notch, the cat on and off.
    fn slot_cases() -> Vec<(Style, NotchSize, bool)> {
        let mut out = Vec::new();
        for style in [Style::Pill, Style::Notch] {
            for w in [NOTCH_W_MIN, NOTCH_W_DEFAULT, NOTCH_W_MAX] {
                for h in [NOTCH_H_MIN, NOTCH_H_DEFAULT, NOTCH_H_MAX] {
                    for robot in [false, true] {
                        out.push((style, NotchSize::from_px(w, h), robot));
                    }
                }
            }
        }
        out
    }

    /// The right edge of the last thing a compact view draws on `content`.
    fn last_element(kind: ShapeKind, content: ShapeRect, robot: bool) -> f32 {
        use super::super::panel_layout::{pomodoro_compact_layout, pomodoro_compact_layout_robot};
        match (kind, robot) {
            (ShapeKind::MediaCompact, false) => compact_layout(content).bars.right(),
            (ShapeKind::MediaCompact, true) => compact_layout_robot(content).bars.right(),
            (ShapeKind::PomodoroCompact, false) => pomodoro_compact_layout(content).2.right(),
            (ShapeKind::PomodoroCompact, true) => pomodoro_compact_layout_robot(content).3.right(),
            (ShapeKind::RecordCompact, robot) => {
                super::super::panel_layout::record_compact_layout(content, robot, false)
                    .system
                    .right()
            }
            (ShapeKind::MediaLyric, false) => lyric_layout(content).right(),
            (ShapeKind::MediaLyric, true) => lyric_layout_robot(content).1.right(),
            _ => unreachable!(),
        }
    }

    /// The compact shape of `kind` and its twin with the slot, and the width
    /// the content has with the slot settled (`toast_slot_parts`'s `base_w`).
    fn slot_pair(
        style: Style,
        size: NotchSize,
        kind: ShapeKind,
        robot: bool,
        lyric_text: f32,
    ) -> (ShapeRect, ShapeRect, f32) {
        if kind == ShapeKind::MediaLyric {
            let max_w = lyric_max_w(1920.0);
            let min_w = size.w;
            let w = lyric_width_min(Some(lyric_text), robot, min_w, max_w);
            let slot_w = lyric_slot_width(w, robot, min_w, max_w);
            return (
                lyric_shape_sized(style, size, w),
                lyric_shape_sized(style, size, slot_w),
                slot_w - TOAST_SLOT_EXTRA,
            );
        }
        let base = shape_with_robot_sized(style, size, kind, robot);
        (base, with_toast_slot(base), base.w)
    }

    #[test]
    fn the_slot_widens_a_compact_shape_and_keeps_it_centred() {
        for (style, size, robot) in slot_cases() {
            for kind in [
                ShapeKind::MediaCompact,
                ShapeKind::PomodoroCompact,
                ShapeKind::RecordCompact,
            ] {
                let base = shape_with_robot_sized(style, size, kind, robot);
                let wide = with_toast_slot(base);
                assert_eq!(wide.w, base.w + TOAST_SLOT_EXTRA);
                assert!(((wide.x + wide.w / 2.0) - (base.x + base.w / 2.0)).abs() < 0.001);
                assert_eq!((wide.y, wide.h, wide.radius), (base.y, base.h, base.radius));
            }
        }
        assert_eq!(TOAST_SLOT_EXTRA, 28.0);
    }

    #[test]
    fn with_the_slot_the_content_ends_8_dip_left_of_the_logo_inside_the_shape() {
        for (style, size, robot) in slot_cases() {
            for kind in [
                ShapeKind::MediaCompact,
                ShapeKind::PomodoroCompact,
                ShapeKind::RecordCompact,
                ShapeKind::MediaLyric,
            ] {
                for text in [1.0, 150.0, 5000.0] {
                    let (_, shape, base_w) = slot_pair(style, size, kind, robot, text);
                    let (content, logo, alpha) = toast_slot_parts(shape, base_w);
                    let tag = format!("{style:?} {size:?} {kind:?} robot {robot} text {text}");
                    assert_eq!(alpha, 1.0, "{tag}");
                    assert!((content.w - base_w).abs() < 0.001, "{tag}");
                    assert_eq!(content.x, shape.x, "{tag}");
                    let last = last_element(kind, content, robot);
                    assert!(
                        logo.x - last >= TOAST_SLOT_GAP - 0.001,
                        "{tag}: content ends {last}, logo starts {}",
                        logo.x
                    );
                    assert_eq!((logo.w, logo.h), (TOAST_SLOT, TOAST_SLOT), "{tag}");
                    assert!(inside(shape, logo), "{tag}: the logo is inside the shape");
                    // It ends COMPACT_INSET from the right and is centred.
                    assert!((shape.x + shape.w - logo.right() - COMPACT_INSET).abs() < 0.001);
                    assert!(((logo.y + logo.h / 2.0) - (shape.y + shape.h / 2.0)).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn through_the_morph_the_content_never_overlaps_a_visible_logo() {
        for (style, size, robot) in slot_cases() {
            for kind in [
                ShapeKind::MediaCompact,
                ShapeKind::PomodoroCompact,
                ShapeKind::RecordCompact,
                ShapeKind::MediaLyric,
            ] {
                let (base, shape, base_w) = slot_pair(style, size, kind, robot, 150.0);
                // Both ways: the slot coming in and going out.
                for (from, to) in [(base, shape), (shape, base)] {
                    for step in 0..=100 {
                        let t = step as f32 / 100.0;
                        let now = lerp_shape(from, to, ease_out_back(t));
                        let (content, logo, alpha) = toast_slot_parts(now, base_w);
                        assert!(content.w >= 0.0 && (0.0..=1.0).contains(&alpha));
                        if alpha > 0.0 {
                            let last = last_element(kind, content, robot);
                            assert!(
                                last <= logo.x + 0.001,
                                "{style:?} {kind:?} robot {robot} t {t}: {last} past {}",
                                logo.x
                            );
                        }
                    }
                }
                // Without the slot nothing is taken from the content.
                let (content, _, alpha) = toast_slot_parts(base, base_w);
                assert_eq!((content, alpha), (base, 0.0));
            }
        }
    }

    #[test]
    fn the_logo_fades_in_only_once_it_clears_the_content() {
        let base = shape_sized(Style::Notch, NotchSize::DEFAULT, ShapeKind::MediaCompact);
        let at = |e: f32| {
            let s = ShapeRect {
                w: base.w + e,
                ..base
            };
            toast_slot_parts(s, base.w).2
        };
        assert_eq!(at(0.0), 0.0);
        assert_eq!(at(TOAST_SLOT), 0.0);
        assert!(at(24.0) > 0.0 && at(24.0) < 1.0);
        assert_eq!(at(TOAST_SLOT_EXTRA), 1.0);
        assert_eq!(at(TOAST_SLOT_EXTRA + 5.0), 1.0, "an overshoot is clamped");
        assert_eq!(at(-9.0), 0.0);
    }

    /// The slot's peek is asked of the pointer: wherever it starts, the full
    /// toast lies under the pointer, so the notch cannot flap between the two
    /// forms. A shape wider than the toast leaves a band at its right end with
    /// no peek (its logo still takes a click).
    #[test]
    fn a_peek_only_starts_where_the_full_toast_covers_the_pointer() {
        for style in [Style::Notch, Style::Pill] {
            let size = NotchSize::DEFAULT;
            let notice = shape_sized(style, size, ShapeKind::Notice);
            // A lyric notch with the slot wider than the toast.
            for w in [372.0, 390.0, 412.0] {
                let shape = lyric_shape_sized(style, size, w);
                let (_, logo, _) = toast_slot_parts(shape, w - TOAST_SLOT_EXTRA);
                let y = logo.y + logo.h / 2.0;
                // The band at the right end: outside the toast, no peek.
                let band = shape.x + shape.w - 2.0;
                assert!(band > notice.x + notice.w, "{style:?} {w}");
                assert!(!toast_peek_reaches(style, size, band, y), "{style:?} {w}");
                // Every point where a peek starts lies inside the drawn toast.
                let mut x = logo.x - 4.0;
                while x < shape.x + shape.w {
                    if toast_peek_reaches(style, size, x, y) {
                        assert!(shape_contains(style, notice, x, y), "{style:?} {w} x {x}");
                    }
                    x += 0.5;
                }
            }
            // A narrow compact shape: the whole logo peeks.
            let base = shape_sized(style, size, ShapeKind::MediaCompact);
            let shape = with_toast_slot(base);
            let (_, logo, _) = toast_slot_parts(shape, base.w);
            let cy = logo.y + logo.h / 2.0;
            for x in [logo.x, logo.x + logo.w / 2.0, logo.right()] {
                assert!(toast_peek_reaches(style, size, x, cy), "{style:?} x {x}");
            }
            // Off the island entirely: never.
            assert!(!toast_peek_reaches(style, size, notice.x - 1.0, cy));
        }
    }

    #[test]
    fn the_lyric_total_with_the_slot_stays_under_the_cap_and_keeps_a_box() {
        for robot in [false, true] {
            for min_w in [0.0, 190.0, 220.0, 300.0] {
                for max_w in [220.0, 300.0, 504.0, 560.0] {
                    let cap = lyric_cap(robot, min_w, max_w);
                    for step in 0..=90 {
                        let text = (step * 10) as f32;
                        let w = lyric_width_min(Some(text), robot, min_w, max_w);
                        let total = lyric_slot_width(w, robot, min_w, max_w);
                        let tag = format!("robot {robot} min {min_w} max {max_w} text {text}");
                        assert!(total <= cap, "{tag}: {total} > {cap}");
                        assert!(total >= w.min(cap - TOAST_SLOT_EXTRA) + TOAST_SLOT_EXTRA);
                        // The line keeps a box (under the content's width).
                        let content_w = total - TOAST_SLOT_EXTRA;
                        assert!(lyric_box_w(content_w, robot) > 0.0, "{tag}");
                        // A line that fits keeps today's box exactly.
                        if w <= cap - TOAST_SLOT_EXTRA {
                            assert_eq!(content_w, w, "{tag}");
                        }
                    }
                }
            }
        }
    }

    /// Jadi muka with the earcups: on every face view the unread dot and its
    /// ring keep clear of the right earcup, inside the shape.
    #[test]
    fn the_unread_dot_clears_the_earcup_on_every_face_view() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::face_pieces;
        use crate::ui::robot::face_unit;
        let mut checked = 0;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for v in face_views(style, size, CatAccessory::Headphone) {
                    let scale = face_scale(v.area, CatAccessory::Headphone);
                    let unit = face_unit(scale);
                    let eyes = (v.area.x + v.area.w / 2.0, v.area.y + v.area.h / 2.0);
                    let cups = face_pieces(CatAccessory::Headphone, v.kind, as_rect(v.shape), eyes, unit);
                    assert!(!cups.is_empty(), "{}", v.name);
                    let ((cx, cy), r) = face_toast_dot(v.shape, unit, v.kind);
                    let reach = r + TOAST_DOT_RING;
                    let (dl, dt, dr, db) = (cx - reach, cy - reach, cx + reach, cy + reach);
                    for p in &cups {
                        let c = p.corners((0.0, 0.0), 0.0);
                        let (l, t, rr, b) = c.iter().fold(
                            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                            |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)),
                        );
                        let apart = dr < l || dl > rr || db < t || dt > b;
                        assert!(apart, "{} {style:?} {size:?}: dot {dl}..{dr} cup {l}..{rr}", v.name);
                    }
                    // Inside the shape, its ring included.
                    for (x, y) in [(dl, cy), (dr, cy), (cx, dt), (cx, db)] {
                        assert!(shape_contains(style, v.shape, x, y), "{} ({x}, {y})", v.name);
                    }
                    // Only as far in as the cup needs.
                    assert!(cx <= toast_dot(v.shape).0.0);
                    checked += 1;
                }
            }
        }
        assert!(checked > 0);
    }

    /// The earcups move with the face (its look and nod, never cut by them):
    /// the unread dot and its ring clear the right one at its furthest in,
    /// the face's full slide and nod. Only the idle notch looks or glances
    /// (`face_side_reach`); the other views nod.
    #[test]
    fn the_toast_dot_clears_the_moving_earcup() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::{CatLook, face_pieces};
        use crate::ui::robot::{Expr, NOD_MAX_DIP, face_pose_in, face_slide, face_unit};
        let look = CatLook { accessory: CatAccessory::Headphone, ..CatLook::default() };
        let vs: Vec<f32> = (0..=8).map(|i| -1.0 + 0.25 * i as f32).collect();
        let mut checked = 0;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for view in face_views(style, size, CatAccessory::Headphone) {
                    let FaceView { name, kind, shape: s, clock, area } = view;
                    let scale = face_scale(area, CatAccessory::Headphone);
                    let unit = face_unit(scale);
                    let centre = centre_of(area);
                    let cups = face_pieces(CatAccessory::Headphone, kind, as_rect(s), centre, unit);
                    let ((cx, cy), r) = face_toast_dot(s, unit, kind);
                    let reach = r + TOAST_DOT_RING;
                    let (dl, dt, dr, db) = (cx - reach, cy - reach, cx + reach, cy + reach);
                    let looks = kind == ShapeKind::Idle;
                    for expr in Expr::ALL {
                        let vs: &[f32] = if looks && expr.follows() { &vs } else { &[0.0] };
                        for &v in vs {
                            for nod in [0.0, NOD_MAX_DIP] {
                                let pose = face_pose_in(
                                    style, s, kind, clock, centre, scale, expr, look,
                                    face_pose(expr, v, nod),
                                );
                                let slide = face_slide(&pose, scale);
                                for p in &cups {
                                    let (l, t, rr, b) = p.corners(slide, pose.nod_dip).iter().fold(
                                        (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                                        |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)),
                                    );
                                    let apart = dr < l || dl > rr || db < t || dt > b;
                                    assert!(
                                        apart,
                                        "{name} {style:?} {size:?} {expr:?} v {v} nod {nod}: dot {dl}..{dr} cup {l}..{rr}"
                                    );
                                    checked += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 10_000, "{checked} cups checked");
    }

    /// With the earcups, the unread dot and its ring sit right of every text
    /// box left of them: the idle clock, the media title, the lyric line, the
    /// pomodoro's time and label, the REC clock and its paused label or
    /// caption line, as laid out in `face_content_wearing`.
    #[test]
    fn the_toast_dot_clears_the_content_text() {
        use crate::theme::CatAccessory;
        use crate::ui::panel_layout::{pomodoro_compact_layout_robot, record_compact_layout};
        use crate::ui::robot::face_unit;
        let hp = CatAccessory::Headphone;
        let mut checked = 0;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for view in face_views(style, size, hp) {
                    let FaceView { name, kind, shape: s, area, .. } = view;
                    let unit = face_unit(face_scale(area, hp));
                    let ((cx, _), r) = face_toast_dot(s, unit, kind);
                    let dot_left = cx - r - TOAST_DOT_RING;
                    let c = face_content_wearing(s, kind, hp);
                    let texts: Vec<(&str, f32)> = match name.as_str() {
                        "idle" => vec![("clock", face_idle_layout(s, true, hp).1.expect("clock").right())],
                        "idle without clock" => Vec::new(),
                        "media" => vec![("title", compact_layout_robot(c).text.right())],
                        "pomodoro" => vec![("time and label", pomodoro_compact_layout_robot(c).3.right())],
                        "record" | "caption" => {
                            let l = record_compact_layout(c, true, name == "caption");
                            vec![("clock", l.time.right()), ("label", l.label.right())]
                        }
                        lyric => {
                            assert!(lyric.starts_with("lyric"), "{lyric}");
                            vec![("line", lyric_layout_robot(c).1.right())]
                        }
                    };
                    for (what, right) in texts {
                        assert!(
                            right < dot_left,
                            "{style:?} {size:?} {name}: the {what} ends at {right}, the dot starts at {dot_left}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 1_000, "{checked}");
    }

    /// `face_content_trim` is what `face_content_wearing` takes off the content's
    /// width: the earcups' room, for the Headphone on a narrow view only.
    #[test]
    fn the_face_content_trim_is_what_the_earcups_take_off_the_content() {
        use crate::theme::CatAccessory;
        for style in [Style::Pill, Style::Notch] {
            for size in notch_sizes() {
                for kind in [
                    ShapeKind::MediaCompact,
                    ShapeKind::MediaLyric,
                    ShapeKind::PomodoroCompact,
                    ShapeKind::RecordCompact,
                    ShapeKind::Idle,
                ] {
                    let s = with_face_extra(shape_with_robot_sized(style, size, kind, true), kind);
                    for &acc in CatAccessory::ALL {
                        let taken = face_content(s, kind).w - face_content_wearing(s, kind, acc).w;
                        let trim = face_content_trim(kind, acc);
                        assert!(
                            (taken - trim).abs() < 1e-3,
                            "{style:?} {size:?} {kind:?} {acc:?}: took {taken}, trim {trim}"
                        );
                        let wants = acc == CatAccessory::Headphone && face_extra(kind) > 0.0;
                        assert_eq!(trim > 0.0, wants, "{kind:?} {acc:?}: {trim}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_unread_dot_sits_8_dip_in_from_the_right_end() {
        for (style, size, robot) in slot_cases() {
            for kind in [
                ShapeKind::Idle,
                ShapeKind::MediaCompact,
                ShapeKind::PomodoroCompact,
            ] {
                let s = shape_with_robot_sized(style, size, kind, robot);
                let ((cx, cy), r) = toast_dot(s);
                assert_eq!(r * 2.0, TOAST_DOT);
                assert_eq!(cx, s.x + s.w - 8.0);
                assert_eq!(cy, s.y + s.h / 2.0);
                assert!(cx + r <= s.x + s.w && cy - r >= s.y && cy + r <= s.y + s.h);
            }
        }
    }

    #[test]
    fn the_update_offers_chips_lie_inside_the_notice_clear_of_each_other_and_the_text() {
        let widths = [LATER_CHIP_W, UPDATE_CHIP_W];
        for style in [Style::Pill, Style::Notch] {
            for size in [NotchSize::DEFAULT] {
                for robot in [false, true] {
                    let n = shape_with_robot_sized(style, size, ShapeKind::Notice, robot);
                    let (l, chips) = notice_actions_layout(n, &widths);
                    let [later, update] = chips[..] else {
                        panic!("two chips");
                    };
                    let tag = format!("{style:?} {size:?} robot {robot}");
                    for chip in [later, update] {
                        assert!(inside(n, chip), "{tag} chip {chip:?}");
                        // As clear of the rounded end as every other chip.
                        assert!(
                            chip.right() <= n.x + n.w - corner_radius(style, n) / 2.0,
                            "{tag} chip {chip:?}"
                        );
                        for text in [l.title, l.body, l.badge] {
                            assert!(!intersects(chip, text), "{tag} {chip:?} {text:?}");
                        }
                    }
                    assert!(!intersects(later, update), "{tag}");
                    // "Nanti" at the left, "Update" last at the right.
                    assert!(later.right() + NOTICE_CHIP_GAP <= update.x + 0.001, "{tag}");
                    assert_eq!((later.w, update.w), (LATER_CHIP_W, UPDATE_CHIP_W));
                    assert_eq!(update.right(), l.action.expect("block").right());
                    // The text stops short of both.
                    assert!(l.title.right() < later.x && l.body.right() < later.x, "{tag}");
                    assert!(l.title.w >= 140.0, "{tag} room for \"Vale 3.11.0 tersedia\"");
                    // The same row as the old single chip: centred.
                    assert_eq!(later.y, update.y);
                    assert!((later.y + later.h / 2.0 - (n.y + n.h / 2.0)).abs() < 0.001);
                }
            }
        }
        // No chips: the layout of a notice without any.
        let n = shape_for(Style::Pill, ShapeKind::Notice);
        let (l, chips) = notice_actions_layout(n, &[]);
        assert!(chips.is_empty());
        assert_eq!(l, notice_layout(n, None));
        // One chip: exactly `notice_layout`'s.
        let (l, chips) = notice_actions_layout(n, &[ACTION_W]);
        assert_eq!(l, notice_layout(n, Some(ACTION_W)));
        assert_eq!(Some(chips[0]), l.action);
    }

    #[test]
    fn the_update_offers_chip_regions_hit_test_in_the_notice() {
        for style in [Style::Pill, Style::Notch] {
            let n = shape_for(style, ShapeKind::Notice);
            let (_, chips) = notice_actions_layout(n, &[LATER_CHIP_W, UPDATE_CHIP_W]);
            let regions = HitRegions {
                shape: Some((style, n)),
                regions: update_offer_regions(chips[0], chips[1]),
                ..HitRegions::default()
            };
            let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let (x, y) = mid(chips[0]);
            assert_eq!(regions.hit(x, y), Some(Hit::UpdateLater), "{style:?}");
            let (x, y) = mid(chips[1]);
            assert_eq!(regions.hit(x, y), Some(Hit::UpdateNow), "{style:?}");
            // Each chip's own corner and the gap between them.
            assert_eq!(
                regions.hit(chips[0].x + 1.0, chips[0].y + 1.0),
                Some(Hit::UpdateLater)
            );
            assert_eq!(
                regions.hit(chips[1].right() - 1.0, chips[1].bottom() - 1.0),
                Some(Hit::UpdateNow)
            );
            let gap_x = chips[0].right() + NOTICE_CHIP_GAP / 2.0;
            assert_eq!(regions.hit(gap_x, chips[0].y + 2.0), Some(Hit::Body));
            // The text side and the badge are the body.
            assert_eq!(regions.hit(n.x + 30.0, n.y + n.h / 2.0), Some(Hit::Body));
            assert_eq!(regions.hit(chips[0].x - 20.0, chips[0].y), Some(Hit::Body));
        }
    }

    #[test]
    fn the_notice_close_rect_stays_inside_the_shape_for_every_style_robot_and_size() {
        let sizes = [
            NotchSize::DEFAULT,
            NotchSize::from_px(NOTCH_W_MIN, NOTCH_H_MIN),
            NotchSize::from_px(NOTCH_W_MAX, NOTCH_H_MAX),
            NotchSize::from_px(NOTCH_W_MIN, NOTCH_H_MAX),
        ];
        for style in [Style::Pill, Style::Notch] {
            for robot in [false, true] {
                for size in sizes {
                    let n = shape_with_robot_sized(style, size, ShapeKind::Notice, robot);
                    let r = notice_close_rect(n);
                    let tag = format!("{style:?} robot={robot} {size:?}");
                    assert!(r.w >= 24.0 && r.h >= 24.0, "{tag}");
                    assert_eq!(r.right(), n.x + n.w - NOTICE_CLOSE_INSET, "{tag}");
                    assert_eq!(r.y + r.h / 2.0, n.y + n.h / 2.0, "{tag}");
                    // Every corner (just inside it) is on the island.
                    for (x, y) in [
                        (r.x, r.y),
                        (r.right() - 0.01, r.y),
                        (r.x, r.bottom() - 0.01),
                        (r.right() - 0.01, r.bottom() - 0.01),
                    ] {
                        assert!(shape_contains(style, n, x, y), "{tag} corner {x},{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_closable_notice_keeps_its_text_and_chip_clear_of_the_close_square() {
        for style in [Style::Pill, Style::Notch] {
            for robot in [false, true] {
                let n = shape_with_robot_sized(style, NotchSize::DEFAULT, ShapeKind::Notice, robot);
                let close = notice_close_rect(n);
                let limit = close.x - NOTICE_CLOSE_GAP;
                for action in [None, Some(ACTION_W), Some(LOG_ACTION_W)] {
                    let tag = format!("{style:?} robot={robot} {action:?}");
                    let l = notice_layout_with(n, action, true);
                    assert!(l.title.right() <= limit, "{tag}");
                    assert!(l.body.right() <= limit, "{tag}");
                    if let Some(chip) = l.action {
                        assert!(chip.right() <= limit, "{tag}");
                        assert!(l.title.right() <= chip.x, "{tag}");
                    }
                    let t = toast_notice_layout_with(n, action, true);
                    assert!(t.eyebrow.right() <= limit, "{tag}");
                    assert!(t.title.right() <= limit, "{tag}");
                    assert!(t.body.right() <= limit, "{tag}");
                    if let Some(chip) = t.action {
                        assert!(chip.right() <= limit, "{tag}");
                    }
                    // Without the ✕ the layouts are the old ones, 18 dip wider.
                    let plain = notice_layout(n, action);
                    assert_eq!(plain, notice_layout_with(n, action, false), "{tag}");
                    assert_eq!(
                        toast_notice_layout(n, action),
                        toast_notice_layout_with(n, action, false)
                    );
                    assert_eq!(plain.title.right() - l.title.right(), 18.0, "{tag}");
                }
            }
        }
    }

    #[test]
    fn the_notice_close_region_hit_tests_apart_from_the_body() {
        for style in [Style::Pill, Style::Notch] {
            for robot in [false, true] {
                let n = shape_with_robot_sized(style, NotchSize::DEFAULT, ShapeKind::Notice, robot);
                let close = notice_close_rect(n);
                let regions = HitRegions {
                    shape: Some((style, n)),
                    regions: notice_close_regions(close),
                    ..HitRegions::default()
                };
                let tag = format!("{style:?} robot={robot}");
                let (cx, cy) = (close.x + close.w / 2.0, close.y + close.h / 2.0);
                assert_eq!(regions.hit(cx, cy), Some(Hit::NoticeClose), "{tag}");
                // The text side is the body, and so is the gap before the ✕.
                let l = toast_notice_layout_with(n, None, true);
                let (tx, ty) = (l.title.x + 10.0, l.title.y + l.title.h / 2.0);
                assert_eq!(regions.hit(tx, ty), Some(Hit::Body), "{tag}");
                assert_eq!(regions.hit(close.x - 3.0, cy), Some(Hit::Body), "{tag}");
            }
        }
    }

    #[test]
    fn the_rec_level_groups_hit_test_apart_from_the_clock_and_the_dot() {
        use super::super::panel_layout::{record_compact_layout, rec_mute_regions};
        use crate::model::RecSources;
        use crate::recorder::Source;
        for style in [Style::Pill, Style::Notch] {
            for robot in [false, true] {
                let s = shape_with_robot_sized(style, NotchSize::DEFAULT, ShapeKind::RecordCompact, robot);
                let l = record_compact_layout(s, robot, false);
                let both = RecSources { mic: true, system: true };
                let regions = HitRegions {
                    shape: Some((style, s)),
                    regions: rec_mute_regions(s, &l, both),
                    ..HitRegions::default()
                };
                let tag = format!("{style:?} robot={robot}");
                let cy = s.y + s.h / 2.0;
                let mic = regions.hit(l.mic.x + l.mic.w / 2.0, cy);
                let system = regions.hit(l.system.x + l.system.w / 2.0, cy);
                assert_eq!(mic, Some(Hit::RecMuteCompact(Source::Mic)), "{tag}");
                assert_eq!(system, Some(Hit::RecMuteCompact(Source::System)), "{tag}");
                // The clock and the dot are the body.
                let clock = regions.hit(l.time.x + 4.0, cy);
                assert_eq!(clock, Some(Hit::Body), "{tag}");
                assert_eq!(regions.hit(l.dot.0.0, cy), Some(Hit::Body), "{tag}");
            }
        }
    }

    #[test]
    fn the_reminders_done_chip_region_hit_tests_in_the_notice() {
        let intersects = |a: Rect, b: Rect| {
            a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
        };
        for style in [Style::Pill, Style::Notch] {
            for robot in [false, true] {
                let n = shape_with_robot_sized(style, NotchSize::DEFAULT, ShapeKind::Notice, robot);
                let l = notice_layout(n, Some(ACTION_W));
                let chip = l.action.unwrap();
                let regions = HitRegions {
                    shape: Some((style, n)),
                    regions: reminder_done_regions(chip),
                    ..HitRegions::default()
                };
                let tag = format!("{style:?} robot={robot}");
                let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
                let (x, y) = mid(chip);
                assert_eq!(regions.hit(x, y), Some(Hit::ReminderDone), "{tag}");
                assert_eq!(
                    regions.hit(chip.x - 2.0, y),
                    Some(Hit::ReminderDone),
                    "{tag}"
                );
                let (x, y) = mid(l.badge);
                assert_eq!(regions.hit(x, y), Some(Hit::Body), "{tag}");
                let (x, y) = mid(l.title);
                assert_eq!(regions.hit(x, y), Some(Hit::Body), "{tag}");
                let grown = reminder_done_regions(chip)[0].rect;
                for (name, r) in [("title", l.title), ("body", l.body), ("badge", l.badge)] {
                    assert!(!intersects(grown, r), "{tag}: grown rect meets {name}");
                }
            }
        }
    }

    #[test]
    fn the_update_strips_parts_lie_inside_the_waiting_strip() {
        use super::super::panel_layout::{update_strip_parts, waiting_strip};
        for style in [Style::Pill, Style::Notch] {
            let panel = shape_sized(style, NotchSize::DEFAULT, ShapeKind::Panel);
            let strip = waiting_strip(panel);
            let parts = update_strip_parts(strip);
            let within = |r: Rect| {
                r.x >= strip.x
                    && r.right() <= strip.right() + 0.001
                    && r.y >= strip.y
                    && r.bottom() <= strip.bottom() + 0.001
            };
            for r in [parts.dot, parts.text, parts.later, parts.update] {
                assert!(within(r), "{style:?} {r:?}");
            }
            assert!(!intersects(parts.later, parts.update));
            assert!(!intersects(parts.text, parts.later));
            assert!(!intersects(parts.text, parts.update));
            assert!(!intersects(parts.dot, parts.text));
            assert!(parts.later.right() < parts.update.x, "Nanti then Update");
            assert!(parts.update.right() <= strip.right() - 4.0 + 0.001);
            // The chips are centred in the strip's height.
            for chip in [parts.later, parts.update] {
                assert!((chip.y + chip.h / 2.0 - (strip.y + strip.h / 2.0)).abs() < 0.001);
            }
            // Room for "Vale 3.11.0 is available" with its date of install.
            assert!(parts.text.w >= 260.0, "{style:?} {:?}", parts.text);
        }
    }

    #[test]
    fn the_toast_strip_parts_lie_inside_the_waiting_strip() {
        for style in [Style::Pill, Style::Notch] {
            let panel = shape_sized(style, NotchSize::DEFAULT, ShapeKind::Panel);
            let strip = super::super::panel_layout::waiting_strip(panel);
            let (logo, text) = toast_strip_parts(strip);
            let within = |r: Rect| {
                r.x >= strip.x
                    && r.right() <= strip.right() + 0.001
                    && r.y >= strip.y
                    && r.bottom() <= strip.bottom() + 0.001
            };
            assert!(within(logo) && within(text));
            assert_eq!((logo.w, logo.h), (16.0, 16.0));
            assert_eq!(logo.x, strip.x + 6.0);
            assert!(((logo.y + 8.0) - (strip.y + strip.h / 2.0)).abs() < 0.001);
            assert_eq!(text.x, strip.x + 28.0);
            assert!((strip.right() - text.right() - 10.0).abs() < 0.001);
            assert!(text.x >= logo.right(), "the text clears the logo");
        }
    }
}
