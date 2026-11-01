//! Layout math for the island, in logical pixels (96 DPI). Pure functions, so
//! the renderer only draws what this module decides.

use super::dock::DockZone;
use super::icons::Icon;
use super::panel_layout::Region;
use crate::config::Style;
use crate::i18n::Key;
use crate::model::{Hit, Notice, NoticeKind};
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
    /// The closed island shrunk to the cat's two eyes while the pointer is
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

/// The shrunk notch (`ShapeKind::Eyes`): just room for the cat's two eyes.
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

pub fn idle_layout(s: ShapeRect, robot: bool, clock: bool) -> IdleLayout {
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
            robot: Some(square(s.x + (s.w - ROBOT_COMPACT) / 2.0, y, ROBOT_COMPACT)),
            clock: None,
        },
        (true, true) => {
            let group = ROBOT_COMPACT + ROBOT_GAP + CLOCK_W;
            let x = s.x + (s.w - group) / 2.0;
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
        _ => ACTION_W,
    }
}

/// `action`: room for a chip this wide naming what a click does at the
/// right, the text stopping short of it.
pub fn notice_layout(s: ShapeRect, action: Option<f32>) -> NoticeLayout {
    // The robot takes exactly the badge's place.
    const BADGE: f32 = ROBOT_NOTICE;
    const PAD_L: f32 = 18.0;
    const PAD_R: f32 = 20.0;
    const ACTION_H: f32 = 24.0;
    const ACTION_GAP: f32 = 10.0;
    let badge = square(s.x + PAD_L, s.y + (s.h - BADGE) / 2.0, BADGE);
    let left = badge.right() + 12.0;
    let mid = s.y + s.h / 2.0;
    let edge = s.x + s.w - PAD_R;
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
pub fn toast_notice_layout(s: ShapeRect, action: Option<f32>) -> ToastNoticeLayout {
    let base = notice_layout(s, action);
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
        NoticeKind::UpdateInfo => tint(p.sage, Icon::Check),
        // Drawn by the robot; this badge only shows if it is turned off meanwhile.
        NoticeKind::Greeting { .. } => tint(p.accent, Icon::Sparkle),
        // With the robot off: the Clipboard icon, as on the Pintasan tile.
        NoticeKind::Copied => tint(p.accent, Icon::Clipboard),
        // Text and files with the robot off, or an image whose entry is gone.
        NoticeKind::ClipPreview { .. } => tint(p.accent, Icon::Clipboard),
        // With the robot off: the Camera, as on the Pintasan tile.
        NoticeKind::Screenshot { .. } => tint(p.accent, Icon::Camera),
        NoticeKind::ShotSaved => tint(p.sage, Icon::Download),
        NoticeKind::ShotDeleted => tint(crate::theme::SOFT_RED, Icon::Trash),
        NoticeKind::Pomodoro => tint(p.phase(pomo_next), Icon::Timer),
        NoticeKind::Reminder { .. } => BadgeStyle {
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
        // The click installs the update, or opens the releases page.
        NoticeKind::UpdateAvailable { .. } => Some(Key::UpdateButton),
        NoticeKind::UpdateFailed => Some(Key::SeeReleases),
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

    fn intersects(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    #[test]
    fn robot_and_content_share_every_compact_shape() {
        for style in [Style::Pill, Style::Notch] {
            let head_inside = |s: ShapeRect, robot: Rect| {
                for (x, y) in head_outline(robot) {
                    assert!(
                        shape_contains(style, s, x, y),
                        "{style:?} {robot:?} ({x}, {y})"
                    );
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
        let gx = if mark { gx + pose.eye_dx_dip / k } else { gx };
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
            EAR_STROKE, Expr, HEAD, NOTE_MARGIN_DIP, Part, Pose, Side, ear_drawn, face,
            mark_bounds, with_note_lift,
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
        // The head's outline in grid units, grown by `grow` (the Black coat's
        // rim), and the ears' round strokes (their stroke plus `grow`) round
        // their turned corners.
        let outline = |f: &crate::ui::robot::Face, grow: f32| {
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
        };
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
                                let pose = with_note_lift(
                                    Pose::default().dance(amount, rise, 0.0),
                                    expr,
                                    area,
                                    s.y,
                                );
                                let what = format!(
                                    "{style:?} {size:?} {expr:?} {lyric_w:?} amount {amount} rise {rise}"
                                );
                                // The head and ears, bare and with the Black rim.
                                for grow in [0.0, RIM_WIDTH] {
                                    for p in outline(&f, grow) {
                                        let (x, y) = drawn(area, &f, pose, p, false);
                                        assert!(
                                            shape_contains(style, s, x, y),
                                            "{what} rim {grow}: ({x}, {y}) leaves the shape"
                                        );
                                    }
                                }
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
                                // Every accessory's boxes.
                                for acc in CatAccessory::ALL {
                                    for &(p, eyes) in &accessory_corners(*acc, f.ears) {
                                        let (x, y) = drawn(area, &f, pose, p, eyes);
                                        assert!(
                                            shape_contains(style, s, x, y),
                                            "{what} {acc:?}: ({x}, {y}) leaves the shape"
                                        );
                                    }
                                }
                                checked += 1;
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
        assert_eq!(checked, 2 * 15 * 9 * 5 * 6);
    }

    #[test]
    fn the_looking_cat_stays_in_the_idle_island() {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::accessory_corners;
        use crate::ui::robot::{EAR_STROKE, Expr, GRID, Pose, Side, ear_drawn, face, mark_bounds};
        // Every face the idle view shows (`choose_expr`), open-eyed and blinking.
        let faces = [Expr::Idle, Expr::Happy, Expr::Think, Expr::Sad, Expr::Sleep];
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
                        // The head's outline, in grid units.
                        let head: Vec<(f32, f32)> = head_outline(area)
                            .into_iter()
                            .take(36)
                            .map(|(x, y)| ((x - area.x) / k, (y - area.y) / k))
                            .collect();
                        for (expr, blink) in faces.iter().flat_map(|&e| [(e, false), (e, true)]) {
                            let f = face(expr, blink);
                            // The ears' round strokes around their turned corners.
                            let mut cat = head.clone();
                            for side in [Side::Left, Side::Right] {
                                for (x, y) in ear_drawn(side, f.ears) {
                                    cat.extend((0..16).map(|i| {
                                        let a = (i as f32 * 22.5).to_radians();
                                        let r = EAR_STROKE / 2.0;
                                        (x + r * a.cos(), y + r * a.sin())
                                    }));
                                }
                            }
                            // Every mark's box corners: the eyes and what floats beside the head.
                            let marks: Vec<(f32, f32)> = f
                                .parts
                                .iter()
                                .flat_map(|part| {
                                    let (l, t, r, b) = mark_bounds(part);
                                    [(l, t), (r, t), (r, b), (l, b)]
                                })
                                .collect();
                            for v in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                                let pose = Pose::default().looking_as(expr, v);
                                let at = |p, mark| drawn(area, &f, pose, p, mark);
                                for &p in &cat {
                                    let (x, y) = at(p, false);
                                    assert!(
                                        shape_contains(style, s, x, y),
                                        "{style:?} {size:?} {expr:?} v {v}: ({x}, {y}) leaves the island"
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
                                            "{style:?} {size:?} {expr:?} blink {blink} v {v}: ({x}, {y}) reaches the clock at {}",
                                            c.x
                                        );
                                    }
                                }
                            }
                            // Every accessory's boxes (a piece's corners, the ear ones
                            // turned and lifted with their ear, the glasses on the eyes)
                            // under the same pose: inside the island, left of the clock.
                            for acc in CatAccessory::ALL {
                                let corners = accessory_corners(*acc, f.ears);
                                for v in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                                    let pose = Pose::default().looking_as(expr, v);
                                    for &(p, eyes) in &corners {
                                        let (x, y) = drawn(area, &f, pose, p, eyes);
                                        assert!(
                                            shape_contains(style, s, x, y),
                                            "{style:?} {size:?} {acc:?} {expr:?} v {v}: ({x}, {y}) leaves the island"
                                        );
                                        if let Some(c) = l.clock {
                                            assert!(
                                                x < c.x,
                                                "{style:?} {size:?} {acc:?} {expr:?} v {v}: ({x}, {y}) reaches the clock at {}",
                                                c.x
                                            );
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
        use crate::ui::robot::{eyes_boxes_at, eyes_centre, eyes_offset_in};
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
        let inside = |style, s: ShapeRect, boxes: &[(f32, f32, f32, f32)]| {
            boxes.iter().all(|&(l, t, r, b)| {
                [(l, t), (r, t), (r, b), (l, b)]
                    .into_iter()
                    .all(|(x, y)| island_contains(style, s, x, y, 0.0))
            })
        };
        for style in [Style::Pill, Style::Notch] {
            let pill = shape_sized(style, NotchSize::DEFAULT, ShapeKind::Eyes);
            // The final pill takes the look's extremes as they are: the clamp
            // is a guard, not a visible limit.
            for &(dx, dy) in &offsets[..6] {
                assert_eq!(eyes_offset_in(style, pill, dx, dy), (dx, dy), "{style:?}");
                for blink in [false, true] {
                    let boxes = eyes_boxes_at(eyes_centre(pill), dx, dy, blink);
                    assert!(inside(style, pill, &boxes), "{style:?} ({dx}, {dy}) {blink}");
                }
            }
            // Past the extremes the pill does cut the slide back (down
            // first, then across what is left of the room).
            let (cx, cy) = eyes_offset_in(style, pill, 20.0, 10.0);
            assert!(cx > 0.0 && cx < 20.0 && cy > ly && cy < 10.0, "{style:?} {cx} {cy}");
            assert!(eyes_offset_in(style, pill, 20.0, 0.0).0 > lx);
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
                                let (ox, oy) = eyes_offset_in(style, s, dx, dy);
                                // Never past what was asked, never the other way.
                                assert!(ox.abs() <= dx.abs() && ox * dx >= 0.0, "{ox} {dx}");
                                assert!(oy.abs() <= dy.abs() && oy * dy >= 0.0, "{oy} {dy}");
                                for blink in [false, true] {
                                    let boxes = eyes_boxes_at(eyes_centre(s), ox, oy, blink);
                                    assert!(
                                        inside(style, s, &boxes),
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
    fn the_update_notices_have_chips_and_badges() {
        use crate::theme::{Accent, Surface, island};
        let p = island(Accent::Plum, Surface::Espresso);
        let available = NoticeKind::UpdateAvailable {
            version: "3.11.0".into(),
        };
        assert_eq!(
            notice_action(&available, PomoStatus::Idle),
            Some(Key::UpdateButton)
        );
        assert_eq!(
            notice_action(&NoticeKind::UpdateFailed, PomoStatus::Idle),
            Some(Key::SeeReleases)
        );
        for kind in [NoticeKind::UpdateProgress { pct: 5 }, NoticeKind::UpdateInfo] {
            assert_eq!(notice_action(&kind, PomoStatus::Idle), None);
        }
        assert_eq!(notice_action_w(&available), ACTION_W);
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

    const ALL_KINDS: [ShapeKind; 9] = [
        ShapeKind::Idle,
        ShapeKind::MediaCompact,
        ShapeKind::MediaLyric,
        ShapeKind::PomodoroCompact,
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
}
