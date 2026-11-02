//! Drawing of the open panel's pages, the compact pomodoro, the waiting strip
//! and the page dots. A child module of `render`, so it shares the renderer's
//! private state and drawing helpers. Positions come from `panel_layout`;
//! colours only from the island `Palette`, radii from `theme::radius`.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_ELLIPSE, D2D1_LAYER_OPTIONS_NONE,
    D2D1_LAYER_PARAMETERS, D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1Bitmap,
    ID2D1Brush, ID2D1Geometry,
};
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::core::{Interface, Result};
use windows_numerics::{Matrix3x2, Vector2};

use super::{
    CleanerView, CliView, ClipView, DevicesView, FolderMenuView, Frame, GroupShown, NotifView,
    Progress, Renderer, RobotFrame, RowSize, SearchView, SystemView, ToastBadgeView, color,
    d2d_rect, mix,
};
use crate::audio::{self, MixerRow, Target};
use crate::calendar::{CalEvent, CalStatus, EventTime, events_on, range_for};
use crate::clean::{CleanCat, CleanMode};
use crate::cli_folders::{MenuItem, RowNote, Section};
use crate::cli_panel::{LineKind, RunState, echo_command, line_kind, visible_range};
use crate::clipboard::image::Thumb;
use crate::clipboard::kinds::hex_rgb;
use crate::clipboard::store::ClipImage;
use crate::clipboard::{Clip, ClipData, ClipKind};
use crate::config::Style;
use crate::devices::{self, DeviceKind};
use crate::i18n::{self, Key, Lang, text};
use crate::model::{CalView, Hit, MediaInfo, Notice, NoticeKind, Page};
use crate::mole::{MoleGroup, MoleOpt};
use crate::notes::Note;
use crate::notif::NotifEntry;
use crate::notif::tile::{tile_color, tile_letter};
use crate::notif::time::relative_time;
use crate::pomodoro::{Phase, PomoStatus, Pomodoro};
use crate::reminders::{Reminder, Repeat, local_time};
use crate::search::Kind as SearchKind;
use crate::shelf::ShelfCard;
use crate::shortcuts;
use crate::state::MolePhase;
use crate::system::{self, SystemSample};
use crate::theme::{self, Palette, radius};
use crate::tiles::Tile;
use crate::ui::dock::{self, DOCK_ICON};
use crate::ui::geometry::{
    CLAUDE, ExpandedLayout, Rect, ShapeRect, panel_kind, shape_for, toast_strip_parts,
};
use crate::ui::icons::Icon;
use crate::ui::month_grid::{CellCanvas, CellLook, DayState, Filled, day_geometry, paint_day};
use crate::ui::motion::{ease_out_cubic, page_fade_frame, panel_fade, panel_regions_wanted};
use crate::ui::panel_layout::{
    ArrangeLayout, ArrangeShape, CLEAN_CHECK, CLEANER_READY, CLI_LINE_H, CLI_TERM_RADIUS,
    CLIP_RADIUS, CLIP_SCROLLBAR_W, CLIP_THUMB_W, CalendarLayout, CleanFoot, CleanHead,
    CleanerLayout, CliHead, CliLayout, ClipButton, ClipLayout, DEV_BAR_H, DevicesHead,
    DevicesLayout, HOUR_GUTTER, ListLayout, MENU_RADIUS, MENU_ROW_RADIUS, MIXER_TRACK_H,
    MOLE_LOG_LINE_H, MOLE_LOG_PAD, MenuLayout, MeterLayout, MixerLayout, MoleFoot, MoleLayout,
    NOTIF_DOT, NOTIF_RADIUS, NotifHead, NotifLayout, PageLayout, PomodoroLayout, RING_STROKE,
    SEG_PAD, SYS_RING_STROKE, SearchLayout, ShelfLayout, ShortcutsLayout, SystemHead, SystemLayout,
    VOLUME_HUD_H, WEEK_HEAD_H, arrange_layout, calendar_layout, cleaner_layout, cli_folder_menu,
    cli_layout, cli_suggestions, clipboard_layout, device_shapes, devices_layout,
    media_panel_layout, media_progress_layout, mixer_layout, mole_layout, notes_layout,
    notif_layout, page_dots, pomodoro_compact_layout, pomodoro_compact_layout_robot,
    pomodoro_layout, regions, reminders_layout, search_layout, shelf_card_layout, shelf_layout,
    shortcuts_layout, system_layout, tile_parts, volume_hud_layout, waiting_strip,
};
use crate::ui::robot::{Expr, Pose};
use crate::ui::shelf_motion::Drawn;

// Susun's page and notice are drawn in a child file.
#[path = "render_arrange.rs"]
pub(super) mod arrange_paint;

/// A heading's line (DM Sans 600 20, 26 px) is taller than the 22 px header
/// row: its box grows by this much above and below so the line, descenders
/// included, is not clipped.
const HEADING_BLEED: f32 = 4.0;
/// Tints over the island: an all-day band, a day view block, a week chip.
const BAND_TINT: f32 = 0.16;
const BLOCK_TINT: f32 = 0.2;
const CHIP_TINT: f32 = 0.14;
/// How far block text moves from the event colour towards the text colour.
const BLOCK_TEXT_MIX: f32 = 0.55;
/// The "Terlewat" tag's fill under its ochre text.
const TAG_TINT: f32 = 0.16;
/// A note card's time, a step quieter than the snippet.
const NOTE_TIME_ALPHA: f32 = 0.75;
/// The round playhead on the Music page's progress bar.
const PROGRESS_KNOB: f32 = 10.0;

/// The CLI page after `.hchip`, `.stop`, `.spin`, `.lockb`, `.sugg` and
/// `.caret` in docs/design/pengingat-cli.html.
const CHIP_PAD_X: f32 = 9.0;
const SPIN: f32 = 12.0;
const SPIN_GAP: f32 = 5.0;
const STOP_SQUARE: f32 = 7.0;
const STOP_GAP: f32 = 5.0;
const STOP_PAD_X: f32 = 10.0;
/// The lock banner hangs from the panel's top edge, this far over the header.
const CLI_LOCK_ABOVE: f32 = 16.0;
const LOCK_H: f32 = 16.0;
const LOCK_PAD_X: f32 = 10.0;
const LOCK_DOT: f32 = 6.0;
/// The dock's backdrop: room above the grown icons and below the row, at
/// the sides, and its corner radius.
const DOCK_PILL_PAD: f32 = 4.0;
const DOCK_PILL_SIDE: f32 = 10.0;
const DOCK_PILL_RADIUS: f32 = 12.0;
const LOCK_GAP: f32 = 5.0;
const LOCK_RADIUS: f32 = 8.0;
/// `›` and the space after it in front of an echoed command.
const CLI_MARK_W: f32 = 14.0;
const SUGGEST_RADIUS: f32 = 12.0;
const SUGGEST_ROW_RADIUS: f32 = 8.0;
const SUGGEST_PAD_X: f32 = 9.0;
const HINT_GAP: f32 = 8.0;
const CARET_W: f32 = 7.0;
const CARET_H: f32 = 14.0;
const FOCUS_W: f32 = 1.5;
/// The "dari Explorer" chip's folder icon and the gap after it.
const CHIP_ICON: f32 = 12.0;
const CHIP_ICON_GAP: f32 = 5.0;
/// The folder menu's rows: the icon, the gaps around the path, the star.
const MENU_ICON: f32 = 13.0;
const MENU_STAR_ICON: f32 = 12.0;
const MENU_IN_X: f32 = 8.0;
const MENU_GAP: f32 = 8.0;
/// The first Explorer row's accent tint (`.it.on`).
const MENU_ON_TINT: f32 = 0.16;
/// The System page: what stands for a figure before the first sample.
const DASH: &str = "\u{2013}";
/// The Devices page's buttons: the glyph, its gap to the label, the padding
/// either side; a disabled one (ejecting) is dimmed; the refusal row's tint.
const DEV_BTN_ICON: f32 = 12.0;
const DEV_BTN_ICON_GAP: f32 = 5.0;
const DEV_BTN_PAD_X: f32 = 12.0;
const DEV_DIM: f32 = 0.5;
const DEV_VETO_TINT: f32 = 0.16;
/// The CPU line (`spark()`): 1.6 px over a 0.14 fill.
const SPARK_STROKE: f32 = 1.6;
const SPARK_FILL: f32 = 0.14;
/// "Bersihkan" (`.cbtn`): 12 px in, a 13 px sparkle and 6 px to the label;
/// dimmed while the cleaner does not exist, with its "segera" tag.
const CLEAN_PAD_X: f32 = 12.0;
const CLEAN_ICON: f32 = 13.0;
const CLEAN_ICON_GAP: f32 = 6.0;
const CLEAN_DIM: f32 = 0.55;
const SOON_GAP: f32 = 6.0;
const SOON_PAD_X: f32 = 6.0;
const SOON_H: f32 = 14.0;
/// The cleaner (deepclean-opsi-d.html): the segment's item padding and
/// corners, the striped rows' tint, the row icon, the checkbox's corner and
/// line, the admin tag, the button's padding, and the dimmed look of what
/// takes no click.
const SEG_ITEM_PAD_X: f32 = 10.0;
const SEG_RADIUS: f32 = 10.0;
const SEG_ITEM_RADIUS: f32 = 8.0;
const ROW_STRIPE: f32 = 0.03;
const ROW_RADIUS: f32 = 10.0;
const ROW_ICON: f32 = 17.0;
const CHECK_RADIUS: f32 = 5.0;
const CHECK_BORDER: f32 = 1.5;
const CHECK_MARK: f32 = 2.0;
const ADMIN_GAP: f32 = 6.0;
const ADMIN_PAD_X: f32 = 6.0;
const ADMIN_H: f32 = 13.0;
const ADMIN_TINT: f32 = 0.15;
const GO_PAD_X: f32 = 12.0;
const BACK_ICON: f32 = 14.0;
const CLEAN_INERT: f32 = 0.5;
/// What stands for a size not measured.
const EM_DASH: &str = "\u{2014}";
/// Mole mode: the ochre "Eksperimental" chip's tint, the log box's and the
/// install card's corners.
const MOLE_CHIP_TINT: f32 = 0.15;
const MOLE_LOG_RADIUS: f32 = 12.0;
const MOLE_CARD_RADIUS: f32 = 14.0;

/// The Mole groups' names and subtitles (`MG` in deepclean-opsi-d.html).
fn mole_group_keys(g: MoleGroup) -> (Key, Key) {
    match g {
        MoleGroup::Browser => (Key::CleanBrowser, Key::MoleBrowserSub),
        MoleGroup::Apps => (Key::CleanApps, Key::MoleAppsSub),
        MoleGroup::Dev => (Key::CleanDev, Key::MoleDevSub),
        MoleGroup::TempLogs => (Key::CleanTempLogs, Key::MoleTempSub),
    }
}

fn mole_group_icon(g: MoleGroup) -> Icon {
    match g {
        MoleGroup::Browser => Icon::Globe,
        MoleGroup::Apps => Icon::Package,
        MoleGroup::Dev => Icon::Code,
        MoleGroup::TempLogs => Icon::TempFile,
    }
}

/// Mole's footer line: "Pratinjau: ± 2,4 GB" (armed: "± 2,4 GB · 29
/// item"), the scan, or that the preview failed; nothing while a clean runs
/// or after it (the result shows above the log).
fn mole_footer(lang: Lang, view: &CleanerView) -> String {
    let m = &view.mole;
    let total = m.summary.and_then(|s| s.total);
    match m.phase {
        MolePhase::Unknown | MolePhase::Detecting | MolePhase::Scanning(_) => format!(
            "{} {}",
            text(lang, Key::MoleScanning),
            text(lang, Key::MoleScanTime)
        ),
        MolePhase::Previewed { ok: false, .. } => text(lang, Key::MolePreviewFailed).to_owned(),
        MolePhase::Previewed { .. } => match total {
            Some((bytes, entries)) if view.armed => i18n::mole_total_items(lang, bytes, entries),
            _ => i18n::mole_preview(lang, total.map(|t| t.0)),
        },
        MolePhase::Missing | MolePhase::Running | MolePhase::Finished { .. } => String::new(),
    }
}

/// Mole's button: "Jalankan Mole", armed "Yakin? Hapus permanen", running,
/// or "Pratinjau lagi" after a run.
fn mole_go_label(lang: Lang, view: &CleanerView) -> &'static str {
    let key = match view.mole.phase {
        MolePhase::Running => Key::MoleRunning,
        MolePhase::Finished { .. } => Key::MoleAgain,
        _ if view.armed => Key::MoleConfirm,
        _ => Key::MoleRun,
    };
    text(lang, key)
}

/// The line over Mole's log after a run: what was freed, measured, or how
/// it failed.
fn mole_result(lang: Lang, view: &CleanerView) -> (String, bool) {
    let ok = matches!(view.mole.phase, MolePhase::Finished { ok: true });
    match view.mole.report {
        Some(r) if ok => (i18n::mole_result(lang, r.freed, r.drive), true),
        Some(r) => (i18n::mole_failed_title(lang, r.run.exit), false),
        None if ok => (text(lang, Key::MoleAgain).to_owned(), true),
        None => (i18n::mole_failed_title(lang, None), false),
    }
}

/// The cleaner's button label: the total, armed "Yakin? Hapus …", or the
/// clean running.
fn go_label(lang: Lang, view: &CleanerView) -> String {
    let total = view.picked_bytes();
    if view.busy {
        text(lang, Key::CleanBusy).to_owned()
    } else if view.armed {
        i18n::clean_confirm(lang, total)
    } else {
        i18n::clean_go(lang, total)
    }
}

/// The row icons, from the design's `I` map.
fn clean_icon(cat: CleanCat) -> Icon {
    match cat {
        CleanCat::TempLogs => Icon::TempFile,
        CleanCat::BrowserCache => Icon::Globe,
        CleanCat::AppCache => Icon::Package,
        CleanCat::DevCache => Icon::Code,
        CleanCat::AutodeskAdobe => Icon::Image,
        CleanCat::OldInstallers => Icon::Download,
        CleanCat::WindowsUpdate => Icon::Windows,
    }
}

/// The uptime chip's text; "Uptime –" before the first sample.
fn system_uptime(lang: Lang, sample: Option<&SystemSample>) -> String {
    sample.map_or_else(
        || format!("Uptime {DASH}"),
        |s| i18n::uptime(lang, s.uptime),
    )
}

/// How a CLI header chip looks: the folder (path ellipsis) and the shell in
/// mono on the raised fill; the run's state tinted; "dari Explorer" in the
/// accent with a folder icon.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ChipLook {
    Explorer,
    Mono,
    Path,
    Run,
    Ok,
    Err,
}

#[derive(Clone, Debug, PartialEq)]
struct CliChip {
    text: String,
    look: ChipLook,
}

/// The header chips left to right: folder and shell; "Berjalan · 0:42"
/// while a command runs (Stop follows it); folder, the result and the shell
/// once one ended. "dari Explorer" comes first while the folder is the one
/// the page followed from Explorer (and not while a command runs).
fn cli_chips(lang: Lang, cli: &CliView, now: Instant) -> Vec<CliChip> {
    let chip = |text: String, look| CliChip { text, look };
    let folder = chip(cli.cwd.to_owned(), ChipLook::Path);
    let shell = chip(cli.shell.name().to_owned(), ChipLook::Mono);
    let explorer = cli.from_explorer.then(|| {
        chip(
            text(lang, Key::CliFromExplorer).to_owned(),
            ChipLook::Explorer,
        )
    });
    match cli.run {
        RunState::Idle => explorer.into_iter().chain([folder, shell]).collect(),
        RunState::Running { since } => {
            let elapsed = now.saturating_duration_since(since);
            vec![chip(i18n::cli_running(lang, elapsed), ChipLook::Run)]
        }
        RunState::Ended { code, killed, took } => {
            let look = if code == Some(0) && !killed {
                ChipLook::Ok
            } else {
                ChipLook::Err
            };
            let result = chip(i18n::cli_result(lang, code, killed, took), look);
            explorer
                .into_iter()
                .chain([folder, result, shell])
                .collect()
        }
    }
}

/// The first `n` UTF-16 units of `s` (an EDIT's offsets), never splitting a
/// character.
fn utf16_prefix(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// A track time as the player shows it: "m:ss", or "h:mm:ss" from an hour.
fn track_time(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

impl Renderer {
    /// A page title in the header row.
    fn heading(&self, title: &str, header: Rect) {
        let area = Rect {
            y: header.y - HEADING_BLEED,
            h: header.h + 2.0 * HEADING_BLEED,
            ..header
        };
        self.text(title, &self.fonts.heading, area, 1.0);
    }

    /// The panel: content laid out for the resting panel shape, clipped to the
    /// current (possibly still morphing) shape and faded in while it grows.
    /// Clickable regions exist only once the shape has settled and no page
    /// is sliding.
    pub(super) fn paint_panel(
        &mut self,
        style: Style,
        shape: ShapeRect,
        frame: &Frame,
    ) -> Result<()> {
        let kind = panel_kind(frame.page, frame.cli.tall, frame.cleaner.is_some());
        let rest = shape_for(style, kind);
        let settled = shape == rest;
        // Content is fully visible by ~70 % of the growth, so the panel reads
        // as open while the shape is still easing into place; closing, it
        // fades the same way (`paint_leaving_panel`).
        let fade = if settled {
            1.0
        } else {
            panel_fade(shape.h, rest.h, frame.transit.fade_floor_h)
        };
        if fade <= 0.0 {
            return Ok(());
        }
        let mask = self.shape_geometry(style, shape)?;
        self.push_mask(&mask, fade);
        let painted = self.paint_panel_content(rest, frame, frame.page, settled, false);
        self.set_offset(0.0);
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe { target.rt.PopLayer() };
        painted
    }

    /// The panel that is closing, drawn inside the shape that shrinks around
    /// it, fading with the shape's height down to the height of the view it
    /// shrinks into (`Transit::fade_floor_h`), so it is gone exactly when the
    /// morph ends. It takes no clicks and slides or
    /// reveals nothing.
    pub(super) fn paint_leaving_panel(
        &mut self,
        style: Style,
        shape: ShapeRect,
        frame: &Frame,
        page: Page,
    ) -> Result<()> {
        // Sized as it showed (`frame` carries what it showed, not the reset).
        let rest = shape_for(
            style,
            panel_kind(page, frame.cli.tall, frame.cleaner.is_some()),
        );
        let fade = panel_fade(shape.h, rest.h, frame.transit.fade_floor_h);
        if fade <= 0.0 {
            return Ok(());
        }
        let mask = self.shape_geometry(style, shape)?;
        self.push_mask(&mask, fade);
        let painted = self.paint_panel_content(rest, frame, page, false, true);
        self.set_offset(0.0);
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe { target.rt.PopLayer() };
        painted
    }

    /// One page at `offset` and `alpha`; an opacity layer only below 1.
    fn paint_page_at(
        &mut self,
        frame: &Frame,
        layout: &PageLayout,
        (dx, dy): (f32, f32),
        alpha: f32,
    ) -> Result<()> {
        let layered = alpha < 1.0;
        if layered {
            self.push_opacity(alpha.clamp(0.0, 1.0));
        }
        self.set_translation(dx, dy);
        let painted = self.paint_page(frame, layout);
        self.set_translation(0.0, 0.0);
        if layered {
            let target = self.target.as_ref().expect("target ensured by paint");
            unsafe { target.rt.PopLayer() };
        }
        painted
    }

    /// `closing`: the panel on its way out, shown as it was (no slide, fade
    /// or reveal, no click regions).
    fn paint_panel_content(
        &mut self,
        s: ShapeRect,
        frame: &Frame,
        page: Page,
        settled: bool,
        closing: bool,
    ) -> Result<()> {
        let strip = frame.strip.is_some();
        let layout = self.page_layout(s, frame, page, strip);
        let (slide, fade, reveal) = if closing {
            (0.0, None, None)
        } else {
            (
                frame.page_slide,
                // A cross-fade at its end is a settled page (`t >= 1`).
                frame.transit.page_fade.and_then(page_fade_frame),
                frame.transit.reveal,
            )
        };
        // A toast's strip arriving or leaving: the page content eases down
        // or up with it (not on the panel closing).
        let strip_motion = frame.strip_motion.filter(|_| !closing);
        let strip_dy = strip_motion.map_or(0.0, |m| m.dy);
        let from = frame.page_from.filter(|p| *p != page);
        match (from, fade) {
            // Two pages cross-fade (the search comes and goes over a page).
            (Some(from), Some(t)) => {
                let t = ease_out_cubic(t);
                let old = self.page_layout(s, frame, from, strip);
                self.paint_page_at(frame, &old, (0.0, 0.0), 1.0 - t)?;
                self.paint_page_at(frame, &layout, (0.0, 0.0), t)?;
            }
            _ => {
                if slide != 0.0 {
                    // The pages going out, each a whole panel width from
                    // the one coming in, moving with it.
                    for (out, x) in frame.page_trail.into_iter().flatten() {
                        let old = self.page_layout(s, frame, out, strip);
                        self.paint_page_at(frame, &old, (x * s.w, 0.0), 1.0)?;
                    }
                }
                // A sub-view switch: the new content fades in, a few px
                // from where it will rest.
                // (The CLI's folder menu reveals itself, not the terminal.)
                let reveal = reveal.filter(|_| !matches!(layout, PageLayout::Cli(_)));
                let (alpha, dx, dy) = reveal.map_or((1.0, 0.0, 0.0), |r| (r.alpha, r.dx, r.dy));
                // Off screen early in a long spin: only the pages going out show.
                if slide.abs() < 1.0 {
                    self.paint_page_at(frame, &layout, (slide * s.w + dx, dy + strip_dy), alpha)?;
                }
            }
        }

        if let Some(notice) = frame.strip {
            let alpha = strip_motion.map_or(1.0, |m| m.alpha);
            self.paint_strip(s, notice, frame.strip_badge, alpha)?;
        }
        // The cleaner is not a page: its footer takes the dots' place. The
        // search is not one of the pages the dots count.
        let has_dots = !matches!(
            layout,
            PageLayout::Cleaner(_) | PageLayout::Mole(_) | PageLayout::Search(_)
        );
        if has_dots {
            self.paint_page_dots(s, frame, page);
        }
        // No clicks on a page still sliding in: its controls are not where
        // the regions say yet, and the old page's are still under the pointer.
        // (A strip still moving holds them too, as a slide does.)
        let clicks = panel_regions_wanted(settled, closing, slide, fade) && strip_motion.is_none();
        // The dock's zone stands through a slide or cross-fade (else the row
        // under a still pointer collapses the moment its own click turns
        // the page); only its clicks wait with the page's.
        let mut dock_hits = Vec::new();
        if has_dots && !closing {
            let (zone, hits) = dock::panel_parts(s, &frame.pages, page, frame.dock.e, clicks);
            self.regions.dock = Some(zone);
            dock_hits = hits;
        }
        if clicks {
            // The dock's icons sit over the page's bottom edge: they win.
            let mut hits = dock_hits;
            hits.extend(regions(s, &layout, strip));
            if matches!(layout, PageLayout::Media(_)) && frame.media.is_none() {
                hits.retain(|r| !matches!(r.hit, Hit::Prev | Hit::PlayPause | Hit::Next));
            }
            self.regions.regions = hits;
            // The v1 media buttons must never fire on another page.
            self.regions.buttons = None;
            if let PageLayout::Cli(l) = &layout {
                self.regions.cli_output = Some(Rect {
                    h: l.input.y - l.term.y,
                    ..l.term
                });
            }
            if let PageLayout::Clipboard(l) = &layout {
                self.regions.clip_list = Some(l.list);
            }
            self.regions.notif_list = match &layout {
                PageLayout::Notifications(l) => Some(l.list),
                _ => None,
            };
            self.regions.media_art = match &layout {
                PageLayout::Media(l) => Some(l.art),
                _ => None,
            };
            self.regions.mixer_tracks = match &layout {
                PageLayout::Mixer(l) => l.rows.iter().map(|r| r.track).collect(),
                _ => Vec::new(),
            };
            self.regions.devices_list = match &layout {
                PageLayout::Devices(l) => Some(l.list),
                PageLayout::Arrange(l) => Some(l.list),
                _ => None,
            };
        }
        Ok(())
    }

    /// The page dots, or, as the pointer comes to them, the dock: one icon
    /// per page that magnifies under the pointer (`ui::dock`). `dock.e` lerps
    /// each dot's rect to its resting icon's (`dock::morph_rect`) and
    /// crossfades the two in it; at 0 this is the plain dots.
    fn paint_page_dots(&self, s: ShapeRect, frame: &Frame, page: Page) {
        let (dots, active) = page_dots(s, &frame.pages, page);
        let e = frame.dock.e.clamp(0.0, 1.0);
        let icons = if e > 0.0 {
            dock::resting_layout(s, dots.len())
        } else {
            Vec::new()
        };
        let p = &self.palette;
        if let (Some(first), Some(last)) = (icons.first(), icons.last()) {
            // The backdrop keeps icons that grow over the content legible.
            let peak = (0..dots.len()).fold(1.0f32, |m, i| m.max(frame.dock.scale(i)));
            let top = first.bottom() - DOCK_ICON * peak - DOCK_PILL_PAD;
            let pill = Rect {
                x: first.x - DOCK_PILL_SIDE,
                y: top,
                w: last.right() - first.x + 2.0 * DOCK_PILL_SIDE,
                h: first.bottom() + DOCK_PILL_PAD - top,
            };
            self.set_color(p.surface, e);
            self.fill_round(pill, (pill.h / 2.0).min(DOCK_PILL_RADIUS));
        }
        for (i, dot) in dots.iter().enumerate() {
            let lit = Some(i) == active;
            let Some(icon) = icons.get(i) else {
                self.set_color(if lit { p.accent } else { p.dot_off }, 1.0);
                self.fill_round(*dot, dot.w / 2.0);
                continue;
            };
            // Grows from its bottom edge, around its resting centre x.
            let area = dock::morph_rect(*dot, *icon, e, frame.dock.scale(i));
            if e < 1.0 {
                self.set_color(if lit { p.accent } else { p.dot_off }, 1.0 - e);
                self.fill_round(area, area.w / 2.0);
            }
            let ink = if lit { p.accent } else { p.secondary };
            if let Some(page) = frame.pages.get(i) {
                self.icon_colored(shortcuts::page_icon(*page), area, area.w, ink, e, None);
            }
        }
    }

    /// The room the calendar title takes in the header: its tracked width
    /// plus 1 px of slack (upright DM Sans has no ink past its advance).
    fn calendar_title_w(&self, title: &str) -> f32 {
        self.text_width(title, &self.fonts.heading).ceil() + 1.0
    }

    /// The layout of `page`; the calendar's arrows follow its title, so the
    /// title is measured first.
    fn page_layout(&self, s: ShapeRect, frame: &Frame, page: Page, strip: bool) -> PageLayout {
        match page {
            Page::Calendar => {
                let title = calendar_title(
                    i18n::lang(),
                    frame.cal_view,
                    frame.cal_anchor,
                    frame.cal_selected,
                );
                let title_w = self.calendar_title_w(&title);
                PageLayout::Calendar(calendar_layout(
                    s,
                    frame.cal_view,
                    frame.cal_anchor,
                    frame.cal_selected,
                    &frame.calendar.events,
                    &frame.calendar.status,
                    frame.day_page,
                    strip,
                    title_w,
                    Some(Local::now().naive_local()),
                ))
            }
            Page::Pomodoro => PageLayout::Pomodoro(pomodoro_layout(s, strip)),
            Page::Reminders => PageLayout::Reminders(reminders_layout(
                s,
                frame.reminders,
                frame.reminders_page,
                strip,
                frame.today,
            )),
            Page::Notes => PageLayout::Notes(notes_layout(s, frame.notes, frame.notes_page, strip)),
            Page::Media => PageLayout::Media(media_panel_layout(s, strip)),
            Page::System
                if page == frame.page
                    && frame.cleaner.is_some_and(|c| c.mode == CleanMode::Mole) =>
            {
                let view = frame.cleaner.expect("checked above");
                let head = self.cleaner_head();
                let foot = self.mole_foot(view);
                PageLayout::Mole(mole_layout(s, strip, &head, &foot, view.mole_look()))
            }
            Page::System if page == frame.page && frame.cleaner.is_some() => {
                let view = frame.cleaner.expect("checked above");
                let head = self.cleaner_head();
                let foot = self.cleaner_foot(view);
                let rows_live = !view.busy;
                PageLayout::Cleaner(cleaner_layout(
                    s,
                    strip,
                    &head,
                    &foot,
                    rows_live,
                    view.go_live(),
                ))
            }
            Page::System => {
                let sample = frame.system.and_then(|v| v.sample);
                let head = self.system_head(&system_uptime(i18n::lang(), sample));
                let drives: Vec<char> = sample
                    .map(|s| s.drives.iter().map(|d| d.letter).collect())
                    .unwrap_or_default();
                let procs = sample.map_or(0, |s| s.procs.len());
                PageLayout::System(system_layout(s, strip, &head, &drives, procs))
            }
            Page::Cli => {
                let chips = cli_chips(i18n::lang(), &frame.cli, frame.now);
                let head = self.cli_head(&chips, frame.cli.run.running());
                let mut l = cli_layout(s, strip, &head);
                l.menu = frame.cli.menu.map(|m| cli_folder_menu(&l, &m.items));
                PageLayout::Cli(l)
            }
            Page::Clipboard => {
                let clip = &frame.clip;
                let ids: Vec<u64> = clip.items.iter().map(|c| c.id).collect();
                let image = |id: u64| {
                    clip.items
                        .iter()
                        .any(|c| c.id == id && c.kind == ClipKind::Image)
                };
                PageLayout::Clipboard(clipboard_layout(
                    s,
                    strip,
                    &ids,
                    clip.selected,
                    clip.scroll,
                    image,
                ))
            }
            Page::Notifications => {
                let n = &frame.notif;
                let ids: Vec<u64> = n.items.iter().map(|e| e.id).collect();
                let head = self.notif_head(n);
                PageLayout::Notifications(notif_layout(
                    s,
                    strip,
                    &head,
                    &ids,
                    n.selected,
                    n.scroll,
                    !n.record,
                    n.no_access,
                ))
            }
            Page::Shortcuts => PageLayout::Shortcuts(shortcuts_layout(s, strip, &frame.tiles)),
            // Room for the master row and, under it, a line saying no app
            // plays (or that there is no device).
            Page::Search => PageLayout::Search(search_layout(s, strip, frame.search.results.len())),
            Page::Shelf => PageLayout::Shelf(shelf_layout(s, strip, frame.shelf.len())),
            Page::Devices => {
                let d = &frame.devices;
                let head = self.devices_head(d.list.len());
                let shapes = device_shapes(d.list, d.vetoes, d.expanded);
                PageLayout::Devices(devices_layout(s, strip, &head, &shapes, d.scroll))
            }
            Page::Arrange => {
                let a = &frame.arrange;
                let head = self.arrange_head(a);
                let shape = ArrangeShape {
                    picking: a.picking.is_some(),
                    cards: a.cards.len(),
                    saved: a.saved.len(),
                    windows: a.windows.len(),
                };
                PageLayout::Arrange(arrange_layout(s, strip, &head, shape, a.scroll))
            }
            Page::Mixer => PageLayout::Mixer(mixer_layout(
                s,
                strip,
                frame.mixer.map_or(2, |rows| rows.len().max(2)),
            )),
        }
    }

    fn paint_page(&mut self, frame: &Frame, layout: &PageLayout) -> Result<()> {
        match layout {
            PageLayout::Calendar(l) => {
                self.paint_calendar(l, frame);
                Ok(())
            }
            PageLayout::Pomodoro(l) => self.paint_pomodoro(l, frame),
            PageLayout::Reminders(l) => {
                self.paint_reminders(l, frame.reminders, frame.today);
                Ok(())
            }
            PageLayout::Notes(l) => {
                self.paint_notes(l, frame.notes, frame.today);
                Ok(())
            }
            PageLayout::Media(l) => self.paint_media(
                l,
                frame.media,
                frame.lyrics_on,
                frame.progress,
                frame.volume,
            ),
            PageLayout::System(l) => self.paint_system(l, frame.system),
            PageLayout::Cleaner(l) => {
                if let Some(view) = frame.cleaner {
                    self.paint_cleaner(l, view)?;
                }
                Ok(())
            }
            PageLayout::Mole(l) => {
                if let Some(view) = frame.cleaner {
                    self.paint_mole(l, view);
                }
                Ok(())
            }
            PageLayout::Cli(l) => self.paint_cli(l, frame),
            PageLayout::Clipboard(l) => self.paint_clipboard(l, &frame.clip),
            PageLayout::Notifications(l) => self.paint_notifications(l, &frame.notif),
            PageLayout::Shortcuts(l) => {
                self.paint_shortcuts(l, &frame.tiles);
                Ok(())
            }
            PageLayout::Mixer(l) => {
                self.paint_mixer(l, frame.mixer);
                Ok(())
            }
            PageLayout::Shelf(l) => {
                self.paint_shelf(l, frame);
                Ok(())
            }
            PageLayout::Search(l) => {
                self.paint_search(l, &frame.search);
                Ok(())
            }
            PageLayout::Devices(l) => {
                self.paint_devices(l, &frame.devices);
                Ok(())
            }
            PageLayout::Arrange(l) => {
                self.paint_arrange(l, &frame.arrange);
                Ok(())
            }
        }
    }

    /// The Devices page's header widths: the title, the count chip and
    /// "Eject semua" (its icon, a gap and the label, with padding).
    fn devices_head(&self, count: usize) -> DevicesHead {
        let lang = i18n::lang();
        let title_w = self
            .text_width(text(lang, Key::PageDevices), &self.fonts.heading)
            .ceil()
            + 1.0;
        let chip_w = self
            .text_width(&i18n::devices_connected(lang, count), &self.fonts.small)
            .ceil()
            + 2.0 * CHIP_PAD_X;
        let label = self
            .text_width(text(lang, Key::DevicesEjectAll), &self.fonts.button)
            .ceil();
        DevicesHead {
            title_w,
            chip_w,
            eject_all_w: 2.0 * DEV_BTN_PAD_X + DEV_BTN_ICON + DEV_BTN_ICON_GAP + label,
        }
    }

    /// A rounded button `r` with an optional icon and a label centred
    /// together: `fill` under `ink`, both at `alpha`.
    fn paint_labelled_button(
        &self,
        r: Rect,
        icon: Option<Icon>,
        label: &str,
        (fill, ink): (u32, u32),
        alpha: f32,
    ) {
        self.set_color(fill, alpha);
        self.fill_round(r, r.h / 2.0);
        let label_w = self.text_width(label, &self.fonts.button).ceil();
        let icon_w = icon.map_or(0.0, |_| DEV_BTN_ICON + DEV_BTN_ICON_GAP);
        let mut x = r.x + (r.w - icon_w - label_w) / 2.0;
        if let Some(icon) = icon {
            let area = Rect {
                x,
                w: DEV_BTN_ICON,
                ..r
            };
            self.icon_colored(icon, area, DEV_BTN_ICON, ink, alpha, None);
            x += icon_w;
        }
        let area = Rect {
            x,
            w: label_w + 1.0,
            ..r
        };
        self.text_colored(label, &self.fonts.button, area, ink, alpha);
    }

    /// The Devices page after the design's `dev()`: a card per device (icon
    /// well, title and subtitle, folder / info / Eject buttons, the usage bar
    /// with "used / total"), a red row under a refused eject with "Coba
    /// lagi", the details of an expanded card. The cards are clipped to the
    /// list, which scrolls.
    fn paint_devices(&self, l: &DevicesLayout, view: &DevicesView) {
        let lang = i18n::lang();
        let p = self.palette;
        self.heading(text(lang, Key::PageDevices), l.header);
        if let Some(chip) = l.chip {
            self.set_color(p.raised, 1.0);
            self.fill_round(chip, chip.h / 2.0);
            let count = i18n::devices_connected(lang, view.list.len());
            self.text_colored(&count, &self.fonts.small_center, chip, p.secondary, 1.0);
        }
        if let Some(r) = l.eject_all {
            let alpha = if view.ejecting.is_empty() {
                1.0
            } else {
                DEV_DIM
            };
            let label = text(lang, Key::DevicesEjectAll);
            self.paint_labelled_button(r, Some(Icon::Eject), label, (p.raised, p.text), alpha);
        }
        if let Some(empty) = l.empty {
            let (icon_area, text_area) = (
                Rect {
                    y: empty.y + empty.h / 2.0 - 34.0,
                    h: 30.0,
                    ..empty
                },
                Rect {
                    y: empty.y + empty.h / 2.0 + 2.0,
                    h: 20.0,
                    ..empty
                },
            );
            self.icon_colored(Icon::Usb, icon_area, 26.0, p.secondary, 1.0, None);
            let hint = text(lang, Key::DevicesEmpty);
            self.text_colored(hint, &self.fonts.eleven_center, text_area, p.secondary, 1.0);
        }
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target
                .rt
                .PushAxisAlignedClip(&d2d_rect(l.list), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        for (lay, dev) in l.cards.iter().zip(view.list) {
            self.set_color(p.raised, 1.0);
            self.fill_round(lay.card, radius::CARD);
            self.set_color(p.track, 1.0);
            self.fill_round(lay.well, 10.0);
            let glyph = match dev.kind {
                DeviceKind::Usb => Icon::Usb,
                DeviceKind::Hdd => Icon::Hdd,
                DeviceKind::Sd => Icon::Sd,
            };
            self.icon_colored(glyph, lay.well, 19.0, p.accent, 1.0, None);
            let title = devices::title_line(lang, dev);
            self.text_colored(&title, &self.fonts.body_medium, lay.title, p.text, 1.0);
            let sub = devices::subtitle(lang, dev);
            self.text_colored(&sub, &self.fonts.small, lay.subtitle, p.secondary, 1.0);
            for (r, icon) in [(lay.open, Icon::Folder), (lay.detail, Icon::Info)] {
                self.set_color(p.raised_hi, 1.0);
                self.fill_round(r, r.h / 2.0);
                self.icon_colored(icon, r, DEV_BTN_ICON + 1.0, p.secondary, 1.0, None);
            }
            let ejecting = view.ejecting.contains(&dev.key);
            let key = if ejecting {
                Key::DevicesEjecting
            } else {
                Key::DevicesEject
            };
            let alpha = if ejecting { DEV_DIM } else { 1.0 };
            self.paint_labelled_button(
                lay.eject,
                Some(Icon::Eject),
                text(lang, key),
                (p.accent, p.on_accent),
                alpha,
            );
            // The bar: used over the whole device; a dash before the size is known.
            self.set_color(p.track, 1.0);
            self.fill_round(lay.bar, DEV_BAR_H / 2.0);
            let usage = match devices::used_total(dev) {
                Some((used, total)) => {
                    let fill = Rect {
                        w: lay.bar.w * (used as f32 / total as f32).clamp(0.0, 1.0),
                        ..lay.bar
                    };
                    if fill.w > 0.0 {
                        self.set_color(p.accent, 1.0);
                        self.fill_round(fill, DEV_BAR_H / 2.0);
                    }
                    devices::used_total_text(lang, used, total)
                }
                None => DASH.to_owned(),
            };
            let w = self.text_width(&usage, &self.fonts.mono_small).ceil();
            let area = Rect {
                x: (lay.usage.right() - w).max(lay.usage.x),
                w: w.min(lay.usage.w),
                ..lay.usage
            };
            self.text_colored(&usage, &self.fonts.mono_small, area, p.secondary, 1.0);
            if view.expanded == Some(dev.key.as_str()) && lay.rows.len() >= 3 {
                self.paint_device_details(&lay.rows, dev);
            }
            if let (Some(veto), Some(found)) = (&lay.veto, view.vetoes.get(&dev.key)) {
                let (title, detail) = devices::veto_message(lang, found);
                self.set_color(theme::SOFT_RED, DEV_VETO_TINT);
                self.fill_round(veto.row, radius::BLOCK);
                let title_w = self.text_width(&title, &self.fonts.small_medium).ceil() + 4.0;
                let head = Rect {
                    w: title_w.min(veto.text.w),
                    ..veto.text
                };
                let medium = &self.fonts.small_medium;
                self.text_colored(&title, medium, head, theme::SOFT_RED, 1.0);
                let rest = Rect {
                    x: head.right(),
                    w: (veto.text.w - head.w).max(0.0),
                    ..veto.text
                };
                self.text_colored(&detail, &self.fonts.small, rest, theme::SOFT_RED, 0.9);
                let alpha = if ejecting { DEV_DIM } else { 1.0 };
                self.set_color(p.raised_hi, alpha);
                self.fill_round(veto.retry, veto.retry.h / 2.0);
                let retry = text(lang, Key::DevicesRetry);
                self.text_colored(retry, &self.fonts.small_center, veto.retry, p.text, alpha);
            }
        }
        unsafe { target.rt.PopAxisAlignedClip() };
        if let Some(bar) = l.scrollbar {
            self.set_color(p.secondary, 0.55);
            self.fill_round(bar, CLIP_SCROLLBAR_W / 2.0);
        }
    }

    /// The expanded card's rows: Model, Jalur, Sistem file, then each
    /// partition ("F: · NTFS · 612 GB / 931 GB · label").
    fn paint_device_details(&self, rows: &[(Rect, Rect)], dev: &devices::Device) {
        let lang = i18n::lang();
        let p = self.palette;
        let fs = devices::fs_summary(dev);
        let fixed = [
            (Key::DeviceModel, dev.model.clone()),
            (Key::DeviceConnection, devices::bus_label(lang, dev.bus)),
            (Key::DeviceFileSystem, fs),
        ];
        for ((label, value), (key, shown)) in rows.iter().zip(fixed) {
            self.text_colored(text(lang, key), &self.fonts.small, *label, p.secondary, 1.0);
            let shown = if shown.is_empty() {
                DASH.to_owned()
            } else {
                shown
            };
            self.text_colored(&shown, &self.fonts.small, *value, p.text, 1.0);
        }
        for ((_, value), part) in rows.iter().skip(3).zip(&dev.partitions) {
            let line = devices::partition_line(lang, part);
            self.text_colored(&line, &self.fonts.mono_small, *value, p.text, 1.0);
        }
    }

    /// The Notifikasi header's widths: the title, "n / max", the "Jeda"
    /// label and the clear button (wide enough for "Yakin?" too, so arming
    /// it moves nothing).
    fn notif_head(&self, view: &NotifView) -> NotifHead {
        let lang = i18n::lang();
        let title_w = self
            .text_width(text(lang, Key::PageNotifications), &self.fonts.heading)
            .ceil()
            + 1.0;
        let count = format!("{} / {}", view.items.len(), view.cap);
        let count_w = self.text_width(&count, &self.fonts.small).ceil() + 2.0 * CHIP_PAD_X;
        let pause_label_w = self
            .text_width(text(lang, Key::NotifPause), &self.fonts.small)
            .ceil()
            + 1.0;
        let label = |key: Key| self.text_width(text(lang, key), &self.fonts.button).ceil();
        let clear_w = label(Key::NotifClearAll).max(label(Key::NotifSure))
            + 2.0 * DEV_BTN_PAD_X
            + DEV_BTN_ICON
            + DEV_BTN_ICON_GAP;
        NotifHead {
            title_w,
            count_w,
            pause_label_w,
            clear_w,
        }
    }

    /// An app's logo in `area` (24 px): the picture, contained and rounded,
    /// else a tile in a colour made from the app's name with its first letter.
    fn paint_notif_logo(&self, area: Rect, entry: &NotifEntry, view: &NotifView) -> Result<()> {
        let key = entry.logo_key();
        if let Some(thumb) = view.logos.get(&key)
            && let Some(bitmap) = self.notif_logo(&key, thumb)?
        {
            let target = self.target.as_ref().expect("target ensured by paint");
            unsafe {
                let D2D_SIZE_F { width, height } = bitmap.GetSize();
                let k = (area.w / width.max(1.0)).min(area.h / height.max(1.0));
                let (w, h) = (width * k, height * k);
                let placed = Rect {
                    x: area.x + (area.w - w) / 2.0,
                    y: area.y + (area.h - h) / 2.0,
                    w,
                    h,
                };
                let transform = Matrix3x2 {
                    M11: k,
                    M12: 0.0,
                    M21: 0.0,
                    M22: k,
                    M31: placed.x,
                    M32: placed.y,
                };
                let brush = target.rt.CreateBitmapBrush(&bitmap, None, None)?;
                brush.SetTransform(&transform);
                let brush: ID2D1Brush = brush.cast()?;
                target.rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: d2d_rect(placed),
                        radiusX: 5.0,
                        radiusY: 5.0,
                    },
                    &brush,
                );
            }
            return Ok(());
        }
        self.set_color(tile_color(&entry.source), 1.0);
        self.fill_round(area, 6.0);
        let letter = tile_letter(&entry.source);
        self.text_colored(
            &letter,
            &self.fonts.small_center_bold,
            area,
            self.palette.text,
            1.0,
        );
        Ok(())
    }

    /// The Notifikasi page: the header (title, "n / max", the "Jeda" switch,
    /// "Hapus semua"), the note while saving is off, then a row per toast
    /// (logo, app, time, unread dot; "title — body" under it), the selected
    /// one lit with its three buttons.
    fn paint_notifications(&self, l: &NotifLayout, view: &NotifView) -> Result<()> {
        let lang = i18n::lang();
        let p = self.palette;
        self.heading(text(lang, Key::PageNotifications), l.header);
        if let Some(chip) = l.count {
            self.set_color(p.raised, 1.0);
            self.fill_round(chip, chip.h / 2.0);
            let count = format!("{} / {}", view.items.len(), view.cap);
            self.text_colored(&count, &self.fonts.small_center, chip, p.secondary, 1.0);
        }
        if let (Some(label), Some(switch)) = (l.pause_label, l.pause_switch) {
            let ink = if view.paused { p.text } else { p.secondary };
            self.text_colored(
                text(lang, Key::NotifPause),
                &self.fonts.small,
                label,
                ink,
                1.0,
            );
            self.set_color(if view.paused { p.accent } else { p.track }, 1.0);
            self.fill_round(switch, switch.h / 2.0);
            let d = switch.h - 4.0;
            let knob = Rect {
                x: if view.paused {
                    switch.right() - 2.0 - d
                } else {
                    switch.x + 2.0
                },
                y: switch.y + 2.0,
                w: d,
                h: d,
            };
            self.set_color(
                if view.paused {
                    p.on_accent
                } else {
                    p.secondary
                },
                1.0,
            );
            self.fill_round(knob, d / 2.0);
        }
        if let Some(r) = l.clear {
            if view.armed {
                self.set_color(theme::SOFT_RED, 0.22);
                self.fill_round(r, r.h / 2.0);
                let label = text(lang, Key::NotifSure);
                self.text_colored(label, &self.fonts.button_center, r, theme::SOFT_RED, 1.0);
            } else {
                let label = text(lang, Key::NotifClearAll);
                self.paint_labelled_button(r, Some(Icon::Trash), label, (p.raised, p.text), 1.0);
            }
        }
        if let Some(note) = l.note {
            self.text_colored(
                text(lang, Key::NotifOff),
                &self.fonts.small,
                note,
                p.secondary,
                1.0,
            );
        }
        if let Some(area) = l.empty {
            let mid = area.y + area.h / 2.0;
            let line = |y: f32| Rect { y, h: 18.0, ..area };
            let icon_area = Rect {
                y: mid - 46.0,
                h: 30.0,
                ..area
            };
            self.icon_colored(Icon::Inbox, icon_area, 26.0, p.secondary, 1.0, None);
            let (title, hint) = if view.no_access {
                (Key::NotifNoAccess, None)
            } else if view.paused {
                (Key::NotifEmpty, Some(Key::NotifPaused))
            } else {
                (Key::NotifEmpty, Some(Key::NotifEmptyHint))
            };
            self.text_colored(
                text(lang, title),
                &self.fonts.body_center,
                line(mid - 14.0),
                p.text,
                1.0,
            );
            if let Some(hint) = hint {
                let hint = text(lang, hint);
                self.text_colored(
                    hint,
                    &self.fonts.small_center,
                    line(mid + 6.0),
                    p.secondary,
                    1.0,
                );
            }
        }
        if let Some(r) = l.settings_button {
            let label = text(lang, Key::NotifOpenSettings);
            self.paint_labelled_button(r, None, label, (p.accent, p.on_accent), 1.0);
        }
        if let Some(bar) = l.scrollbar {
            self.set_color(p.secondary, 0.55);
            self.fill_round(bar, CLIP_SCROLLBAR_W / 2.0);
        }

        let mut drawn = Vec::new();
        for row in &l.rows {
            let Some(entry) = view.items.iter().find(|e| e.id == row.id) else {
                continue;
            };
            let selected = view.selected == Some(entry.id);
            self.set_color(if selected { p.raised_hi } else { p.raised }, 1.0);
            self.fill_round(row.row, NOTIF_RADIUS);
            if selected {
                self.stroke_round_width(
                    inset(row.row, 0.75, 0.75),
                    NOTIF_RADIUS - 0.75,
                    p.accent,
                    1.5,
                );
            }
            drawn.push(entry.logo_key());
            self.paint_notif_logo(row.logo, entry, view)?;
            let name_ink = if entry.read { p.secondary } else { p.text };
            match entry.site.as_deref() {
                // A browser's site follows the app, quieter, in what is left.
                Some(site) => {
                    let used = (self
                        .text_width(&entry.source, &self.fonts.body_medium)
                        .ceil()
                        + 1.0)
                        .min(row.name.w);
                    let app = Rect {
                        w: used,
                        ..row.name
                    };
                    self.text_colored(&entry.source, &self.fonts.body_medium, app, name_ink, 1.0);
                    let rest = Rect {
                        x: row.name.x + used,
                        w: row.name.w - used,
                        ..row.name
                    };
                    if rest.w > 0.0 {
                        let tail = format!(" \u{b7} {site}");
                        self.text_colored(&tail, &self.fonts.body_medium, rest, p.secondary, 1.0);
                    }
                }
                None => self.text_colored(
                    &entry.source,
                    &self.fonts.body_medium,
                    row.name,
                    name_ink,
                    1.0,
                ),
            }
            let when = relative_time(entry.at, view.now, lang);
            self.text_colored(&when, &self.fonts.small_right, row.time, p.secondary, 1.0);
            if !entry.read {
                self.set_color(p.accent, 1.0);
                self.fill_round(row.dot, NOTIF_DOT / 2.0);
            }
            let line = if entry.body.is_empty() {
                entry.title.clone()
            } else {
                format!("{} \u{2014} {}", entry.title, entry.body)
            };
            let ink = if entry.read { p.secondary } else { p.text };
            self.text_colored(
                &line,
                &self.fonts.small,
                row.text,
                ink,
                if entry.read { 1.0 } else { 0.9 },
            );
            if let Some(b) = row.buttons {
                let open = text(lang, Key::NotifOpen);
                self.paint_labelled_button(b.open, None, open, (p.accent, p.on_accent), 1.0);
                let mute = text(lang, Key::NotifMute);
                self.paint_labelled_button(b.mute, None, mute, (p.track, p.text), 1.0);
                self.set_color(p.track, 1.0);
                self.fill_round(b.remove, b.remove.h / 2.0);
                self.icon_colored(Icon::X, b.remove, 11.0, p.text, 1.0, None);
            }
        }
        self.notif_logos
            .borrow_mut()
            .retain(|(key, _)| drawn.contains(key));
        Ok(())
    }

    /// The System page's header widths: the uptime chip and "Bersihkan"
    /// (with its "segera" tag until the cleaner exists).
    fn system_head(&self, uptime: &str) -> SystemHead {
        let lang = i18n::lang();
        let uptime_w = self.text_width(uptime, &self.fonts.small).ceil() + 2.0 * CHIP_PAD_X;
        let label = self
            .text_width(text(lang, Key::SystemClean), &self.fonts.button)
            .ceil();
        let tag = if CLEANER_READY {
            0.0
        } else {
            SOON_GAP + self.soon_tag_w(lang)
        };
        let clean_w = 2.0 * CLEAN_PAD_X + CLEAN_ICON + CLEAN_ICON_GAP + label + tag;
        SystemHead { uptime_w, clean_w }
    }

    fn soon_tag_w(&self, lang: Lang) -> f32 {
        self.text_width(text(lang, Key::SystemSoon), &self.fonts.micro)
            .ceil()
            + 2.0 * SOON_PAD_X
    }

    /// keputusan-v3.1.html `sysPanel`: the title, uptime chip and
    /// "Bersihkan"; the CPU ring with the figure and core count, its 60 s
    /// line; RAM (sage) and each drive's meter (ochre past a warning limit,
    /// figures too); "Paling sibuk" and the process rows. Before the first
    /// sample: dashes, empty bars, no rows.
    fn paint_system(&mut self, l: &SystemLayout, view: Option<&SystemView>) -> Result<()> {
        let lang = i18n::lang();
        let p = self.palette;
        let sample = view.and_then(|v| v.sample);
        self.heading(text(lang, Key::System), l.header);

        self.set_color(p.raised, 1.0);
        self.fill_round(l.uptime, l.uptime.h / 2.0);
        let uptime = system_uptime(lang, sample);
        self.text_colored(
            &uptime,
            &self.fonts.small_center,
            l.uptime,
            p.secondary,
            1.0,
        );
        self.paint_clean_button(l.clean, lang);

        let figures = view.and_then(|v| v.eased);
        let cpu = figures.map(|f| f.cpu).or(sample.map(|s| s.cpu));
        self.ring(
            l.ring_center,
            l.ring_radius,
            SYS_RING_STROKE,
            cpu.unwrap_or(0.0) / 100.0,
            p.accent,
        )?;
        let figure = cpu.map_or_else(|| DASH.to_owned(), |c| format!("{c:.0}%"));
        self.text(&figure, &self.fonts.sys_big, l.percent, 1.0);
        let caption = match sample {
            Some(s) => i18n::cpu_cores(lang, s.cores),
            None => "CPU".to_owned(),
        };
        let caption = caption.to_uppercase();
        let ink = p.secondary;
        self.text_colored(&caption, &self.fonts.micro_center, l.caption, ink, 1.0);
        self.paint_spark(l.spark, view.map_or(&[], |v| v.history))?;

        let (disk_limit, ram_limit) =
            view.map_or((0, 100), |v| (v.disk_warn_gb, v.ram_warn_percent));
        let ram = sample.and_then(|s| s.ram_percent().map(|pct| (s, pct)));
        let (value, fraction, warn) = match ram {
            Some((s, pct)) => (
                i18n::ram_usage(lang, s.ram_used, s.ram_total, pct),
                figures.and_then(|f| f.ram).or(Some(f32::from(pct) / 100.0)),
                system::ram_high(pct, ram_limit),
            ),
            None => (DASH.to_owned(), None, false),
        };
        let name = text(lang, Key::SystemRam);
        self.paint_meter(&l.ram, name, &value, fraction, p.sage, warn);
        for (letter, m) in &l.drives {
            let Some(d) = sample.and_then(|s| s.drives.iter().find(|d| d.letter == *letter)) else {
                continue;
            };
            let value = i18n::disk_free(lang, d.free, d.total);
            let fraction = figures
                .and_then(|f| f.drives.iter().find(|(l, _)| l == letter))
                .map_or(f32::from(d.used_percent()) / 100.0, |(_, v)| *v);
            let warn = system::disk_low(d.free, disk_limit);
            let name = i18n::disk_name(*letter);
            self.paint_meter(m, &name, &value, Some(fraction), p.secondary, warn);
        }

        let label = text(lang, Key::SystemBusiest).to_uppercase();
        self.text2(&label, &self.fonts.micro, l.busiest);
        let procs = sample.map_or(&[][..], |s| &s.procs);
        for (row, proc) in l.procs.iter().zip(procs) {
            self.text(&proc.name, &self.fonts.small, row.name, 1.0);
            let cpu = format!("{:.0}%", proc.cpu);
            self.text2(&cpu, &self.fonts.small_right, row.cpu);
            let mem = i18n::format_size(lang, proc.mem);
            self.text2(&mem, &self.fonts.small_right, row.mem);
        }
        Ok(())
    }

    /// `.meter`: the name, the figures right-aligned, the bar on its track
    /// filled `fraction` in `fill`; past a limit the fill and figures go
    /// ochre.
    fn paint_meter(
        &self,
        m: &MeterLayout,
        name: &str,
        value: &str,
        fraction: Option<f32>,
        fill: u32,
        warn: bool,
    ) {
        let p = self.palette;
        self.text(name, &self.fonts.meter_name, m.name, 1.0);
        let ink = if warn { p.ochre } else { p.secondary };
        self.text_colored(value, &self.fonts.eleven_right, m.value, ink, 1.0);
        self.set_color(p.track, 1.0);
        self.fill_round(m.bar, m.bar.h / 2.0);
        if let Some(f) = fraction.filter(|f| *f > 0.0) {
            let bar = Rect {
                w: (m.bar.w * f.min(1.0)).max(m.bar.h),
                ..m.bar
            };
            self.set_color(if warn { p.ochre } else { fill }, 1.0);
            self.fill_round(bar, bar.h / 2.0);
        }
    }

    /// The CPU line: one point per sample, `HISTORY_LEN` across, the newest
    /// at the right; the accent line over a faint fill down to the base.
    fn paint_spark(&self, r: Rect, points: &[f32]) -> Result<()> {
        if points.len() < 2 {
            return Ok(());
        }
        let p = self.palette;
        let step = r.w / (system::HISTORY_LEN - 1) as f32;
        let at = |i: usize, v: f32| {
            let x = r.right() - (points.len() - 1 - i) as f32 * step;
            Vector2::new(x, r.bottom() - v.clamp(0.0, 100.0) / 100.0 * r.h)
        };
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            let area = self.d2d.CreatePathGeometry()?;
            let sink = area.Open()?;
            let first = at(0, points[0]);
            sink.BeginFigure(Vector2::new(first.X, r.bottom()), D2D1_FIGURE_BEGIN_FILLED);
            for (i, v) in points.iter().enumerate() {
                sink.AddLine(at(i, *v));
            }
            sink.AddLine(Vector2::new(r.right(), r.bottom()));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            target.brush.SetColor(&color(p.accent, SPARK_FILL));
            target.rt.FillGeometry(&area, &target.brush, None);

            let line = self.d2d.CreatePathGeometry()?;
            let sink = line.Open()?;
            sink.BeginFigure(first, D2D1_FIGURE_BEGIN_HOLLOW);
            for (i, v) in points.iter().enumerate().skip(1) {
                sink.AddLine(at(i, *v));
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            target.brush.SetColor(&color(p.accent, 1.0));
            target
                .rt
                .DrawGeometry(&line, &target.brush, SPARK_STROKE, &self.round_cap);
        }
        Ok(())
    }

    /// "Bersihkan" (`.cbtn` with the sparkle). Until the cleaner exists it
    /// is the ghost style, dimmed, with a small "segera" tag, and takes no
    /// click (`CLEANER_READY`).
    fn paint_clean_button(&self, r: Rect, lang: Lang) {
        let p = self.palette;
        let (fill, ink, alpha) = if CLEANER_READY {
            (p.accent, p.on_accent, 1.0)
        } else {
            (p.raised, p.text, CLEAN_DIM)
        };
        self.set_color(fill, alpha);
        self.fill_round(r, r.h / 2.0);
        let icon = Rect {
            x: r.x + CLEAN_PAD_X,
            w: CLEAN_ICON,
            ..r
        };
        self.icon_colored(Icon::Sparkle, icon, CLEAN_ICON, ink, alpha, None);
        let label = text(lang, Key::SystemClean);
        let label_w = self.text_width(label, &self.fonts.button).ceil();
        let area = Rect {
            x: icon.right() + CLEAN_ICON_GAP,
            w: label_w + 1.0,
            ..r
        };
        self.text_colored(label, &self.fonts.button, area, ink, alpha);
        if CLEANER_READY {
            return;
        }
        let tag = Rect {
            x: area.x + label_w + SOON_GAP,
            y: r.y + (r.h - SOON_H) / 2.0,
            w: self.soon_tag_w(lang),
            h: SOON_H,
        };
        self.set_color(p.secondary, TAG_TINT);
        self.fill_round(tag, tag.h / 2.0);
        let soon = text(lang, Key::SystemSoon);
        self.text_colored(soon, &self.fonts.micro_center, tag, p.secondary, 1.0);
    }

    /// The cleaner's mode segment, measured: each label and its padding.
    fn cleaner_head(&self) -> CleanHead {
        let lang = i18n::lang();
        let w =
            |key| self.text_width(text(lang, key), &self.fonts.small).ceil() + 2.0 * SEG_ITEM_PAD_X;
        CleanHead {
            seg: [w(Key::CleanModeOwn), w(Key::CleanModeMole)],
        }
    }

    /// The cleaner's footer, measured: "Dipilih …" and the button.
    fn cleaner_foot(&self, view: &CleanerView) -> CleanFoot {
        let lang = i18n::lang();
        let picked = i18n::clean_picked(lang, view.picked_bytes());
        let go = go_label(lang, view);
        CleanFoot {
            picked_w: self.text_width(&picked, &self.fonts.small).ceil() + 2.0,
            go_w: self.text_width(&go, &self.fonts.button).ceil() + 2.0 * GO_PAD_X,
        }
    }

    /// deepclean-opsi-d.html `cleanPanel2` with `segMode(0)`: the back
    /// button and "Bersihkan", the mode segment ("Otomatis · Mole" drawn
    /// but not yet a choice), seven striped rows (checkbox, icon, name and
    /// subtitle, the ochre "admin" tag, the size), the credit line,
    /// "Dipilih" and the button. Dimmed while a clean runs.
    fn paint_cleaner(&self, l: &CleanerLayout, view: &CleanerView) -> Result<()> {
        let lang = i18n::lang();
        let p = self.palette;
        self.paint_clean_head(l.back, l.header, l.seg, l.seg_items, CleanMode::Own);

        let alpha = if view.busy { CLEAN_INERT } else { 1.0 };
        let browsers = view.scan.map_or(&[][..], |s| &s.skipped_browsers[..]);
        for (i, row) in l.rows.iter().enumerate() {
            if i % 2 == 0 {
                self.set_color(p.text, ROW_STRIPE);
                self.fill_round(row.row, ROW_RADIUS);
            }
            let on = view.checked[row.cat.index()];
            self.paint_check(row.check, on, alpha);
            let icon = clean_icon(row.cat);
            self.icon_colored(icon, row.icon, ROW_ICON, p.secondary, alpha, None);
            let (name_key, sub_key) = i18n::clean_cat(row.cat);
            let name = text(lang, name_key);
            self.text_colored(name, &self.fonts.row_name, row.name, p.text, alpha);
            if row.cat.admin() {
                self.paint_admin_tag(row.name, name, alpha);
            }
            let sub = if row.cat == CleanCat::BrowserCache && !browsers.is_empty() {
                i18n::clean_browsers_open(lang, browsers)
            } else {
                text(lang, sub_key).to_owned()
            };
            self.text_colored(&sub, &self.fonts.micro, row.sub, p.secondary, alpha);
            match view.size(row.cat) {
                RowSize::Bytes(b) => {
                    let size = i18n::clean_size(lang, b);
                    self.text_colored(&size, &self.fonts.row_size, row.size, p.text, alpha);
                }
                RowSize::Unknown => {
                    self.text_colored(EM_DASH, &self.fonts.row_size, row.size, p.text, alpha);
                }
                RowSize::Scanning => {
                    let scanning = text(lang, Key::CleanScanning);
                    self.text_colored(
                        scanning,
                        &self.fonts.small_right,
                        row.size,
                        p.secondary,
                        1.0,
                    );
                }
            }
        }

        let credit = text(lang, Key::CleanCredit);
        self.text_colored(credit, &self.fonts.track_time, l.credit, p.secondary, 1.0);
        let picked = i18n::clean_picked(lang, view.picked_bytes());
        self.text_colored(&picked, &self.fonts.small_right, l.picked, p.secondary, 1.0);
        let go_alpha = if l.go_live { 1.0 } else { CLEAN_INERT };
        self.set_color(p.accent, go_alpha);
        self.fill_round(l.go, l.go.h / 2.0);
        let label = go_label(lang, view);
        let center = &self.fonts.button_center;
        self.text_colored(&label, center, l.go, p.on_accent, go_alpha);
        Ok(())
    }

    /// Both modes' header: the back button, "Bersihkan" and the mode
    /// segment (`segMode`) with `mode` lit.
    fn paint_clean_head(
        &self,
        back: Rect,
        header: Rect,
        seg: Rect,
        items: [Rect; 2],
        mode: CleanMode,
    ) {
        let lang = i18n::lang();
        let p = self.palette;
        self.set_color(p.raised, 1.0);
        self.fill_round(back, back.w / 2.0);
        self.icon_colored(Icon::ChevL, back, BACK_ICON, p.secondary, 1.0, None);
        self.heading(text(lang, Key::SystemClean), header);
        self.set_color(p.raised, 1.0);
        self.fill_round(seg, SEG_RADIUS);
        let on = usize::from(mode == CleanMode::Mole);
        self.set_color(p.raised_hi, 1.0);
        self.fill_round(items[on], SEG_ITEM_RADIUS);
        let center = &self.fonts.small_center;
        for (i, key) in [Key::CleanModeOwn, Key::CleanModeMole]
            .into_iter()
            .enumerate()
        {
            let ink = if i == on { p.text } else { p.secondary };
            self.text_colored(text(lang, key), center, items[i], ink, 1.0);
        }
    }

    /// The Mole mode, measured: the chip, the options, protect, the footer
    /// line, the button and "Cara memasang ↗".
    fn mole_foot(&self, view: &CleanerView) -> MoleFoot {
        let lang = i18n::lang();
        let w = |s: &str, format: &IDWriteTextFormat| self.text_width(s, format).ceil();
        let opt = |key| w(text(lang, key), &self.fonts.small) + 2.0;
        MoleFoot {
            chip_w: w(text(lang, Key::MoleChip), &self.fonts.small) + 2.0 * CHIP_PAD_X,
            opt_w: [opt(Key::MoleOptSystem), opt(Key::MoleOptGame)],
            protect_w: w(text(lang, Key::MoleProtect), &self.fonts.button) + 2.0 * GO_PAD_X,
            label_w: w(&mole_footer(lang, view), &self.fonts.small) + 2.0,
            go_w: w(mole_go_label(lang, view), &self.fonts.button) + 2.0 * GO_PAD_X,
            howto_w: w(text(lang, Key::MoleHowTo), &self.fonts.button) + 2.0 * GO_PAD_X,
        }
    }

    /// deepclean-opsi-d.html `molePanel` and d2: the header, the ochre
    /// "Eksperimental" chip and its line (ochre for a version not tested);
    /// the four groups and the two options, Mole's log on the terminal
    /// surface (with the result after a run), or the install card; the
    /// footer with "Lindungi folder…", the preview's total and the button.
    fn paint_mole(&self, l: &MoleLayout, view: &CleanerView) {
        let lang = i18n::lang();
        let p = self.palette;
        let m = &view.mole;
        self.paint_clean_head(l.back, l.header, l.seg, l.seg_items, CleanMode::Mole);

        self.set_color(p.ochre, MOLE_CHIP_TINT);
        self.fill_round(l.chip, l.chip.h / 2.0);
        let chip = text(lang, Key::MoleChip);
        self.text_colored(chip, &self.fonts.small_center, l.chip, p.ochre, 1.0);
        let known = matches!(
            m.phase,
            MolePhase::Scanning(_)
                | MolePhase::Previewed { .. }
                | MolePhase::Running
                | MolePhase::Finished { .. }
        );
        let (intro, ink) = if known {
            let ink = if m.tested { p.secondary } else { p.ochre };
            (i18n::mole_intro(lang, m.version, m.tested), ink)
        } else {
            (text(lang, Key::MoleIntro).to_owned(), p.secondary)
        };
        self.text_colored(&intro, &self.fonts.small, l.intro, ink, 1.0);

        let look = l.look;
        for (i, row) in l.rows.iter().enumerate() {
            if i % 2 == 0 {
                self.set_color(p.text, ROW_STRIPE);
                self.fill_round(row.row, ROW_RADIUS);
            }
            let icon = mole_group_icon(row.group);
            self.icon_colored(icon, row.icon, ROW_ICON, p.secondary, 1.0, None);
            let (name, sub) = mole_group_keys(row.group);
            self.text_colored(
                text(lang, name),
                &self.fonts.row_name,
                row.name,
                p.text,
                1.0,
            );
            self.text_colored(
                text(lang, sub),
                &self.fonts.micro,
                row.sub,
                p.secondary,
                1.0,
            );
            match view.group(row.group) {
                GroupShown::Bytes(b) => {
                    let size = i18n::clean_size(lang, b);
                    self.text_colored(&size, &self.fonts.row_size, row.size, p.text, 1.0);
                }
                GroupShown::SeeLog => {
                    let see = text(lang, Key::MoleSeeLog);
                    self.text_colored(see, &self.fonts.small_right, row.size, p.accent, 1.0);
                }
                GroupShown::Scanning => {
                    let scanning = text(lang, Key::CleanScanning);
                    let f = &self.fonts.small_right;
                    self.text_colored(scanning, f, row.size, p.secondary, 1.0);
                }
            }
        }
        let opt_alpha = if look.opts_live { 1.0 } else { CLEAN_INERT };
        for o in &l.opts {
            self.paint_check(o.check, m.opts.get(o.opt), opt_alpha);
            let key = match o.opt {
                MoleOpt::System => Key::MoleOptSystem,
                MoleOpt::GameMedia => Key::MoleOptGame,
            };
            self.text_colored(
                text(lang, key),
                &self.fonts.small,
                o.label,
                p.secondary,
                opt_alpha,
            );
        }
        if let Some(r) = l.result {
            let (line, ok) = mole_result(lang, view);
            let ink = if ok { p.sage } else { p.ochre };
            self.text_colored(&line, &self.fonts.body_medium, r, ink, 1.0);
        }
        if let Some(log) = l.log {
            self.paint_mole_log(log, m.log, m.phase == MolePhase::Running);
        }
        if let Some(card) = &l.card {
            self.set_color(p.raised, 1.0);
            self.fill_round(card.boxed, MOLE_CARD_RADIUS);
            let think = RobotFrame {
                expr: Expr::Think,
                pose: Pose::default(),
            };
            self.paint_robot(card.robot, think);
            let title = text(lang, Key::MoleMissing);
            self.text_colored(title, &self.fonts.body_medium, card.title, p.text, 1.0);
            let sub = text(lang, Key::MoleMissingSub);
            self.text_colored(sub, &self.fonts.small, card.sub, p.secondary, 1.0);
            self.paint_ghost(card.button, text(lang, Key::MoleHowTo), 1.0);
        }

        if let Some(r) = l.protect {
            self.paint_ghost(r, text(lang, Key::MoleProtect), 1.0);
        }
        let label = mole_footer(lang, view);
        self.text_colored(&label, &self.fonts.small_right, l.label, p.secondary, 1.0);
        if let Some(go) = l.go {
            let alpha = if look.go { 1.0 } else { CLEAN_INERT };
            self.set_color(p.accent, alpha);
            self.fill_round(go, go.h / 2.0);
            let label = mole_go_label(lang, view);
            self.text_colored(label, &self.fonts.button_center, go, p.on_accent, alpha);
        }
    }

    /// `.cbtn.ghost`: a raised pill with the label in the text colour.
    fn paint_ghost(&self, r: Rect, label: &str, alpha: f32) {
        let p = self.palette;
        self.set_color(p.track, alpha);
        self.fill_round(r, r.h / 2.0);
        self.text_colored(label, &self.fonts.button_center, r, p.text, alpha);
    }

    /// Mole's log on the terminal surface (`--term`), DM Mono 9.5, from
    /// the top like the design's; once it is longer than the box, its last
    /// lines (the newest at the bottom). Section headers bright, the rest
    /// quiet.
    fn paint_mole_log(&self, area: Rect, log: &std::collections::VecDeque<String>, running: bool) {
        let p = self.palette;
        self.set_color(theme::TERMINAL_BG, 1.0);
        self.fill_round(area, MOLE_LOG_RADIUS);
        let inner = Rect {
            x: area.x + MOLE_LOG_PAD,
            y: area.y + MOLE_LOG_PAD,
            w: area.w - 2.0 * MOLE_LOG_PAD,
            h: area.h - 2.0 * MOLE_LOG_PAD,
        };
        let rows = (inner.h / MOLE_LOG_LINE_H).floor().max(0.0) as usize;
        let row = |k: usize| Rect {
            y: inner.y + k as f32 * MOLE_LOG_LINE_H,
            h: MOLE_LOG_LINE_H,
            ..inner
        };
        if log.is_empty() && running {
            let lang = i18n::lang();
            let line = text(lang, Key::MoleRunning);
            self.text_colored(
                line,
                &self.fonts.mono_log,
                row(0),
                theme::TERMINAL_HINT,
                1.0,
            );
            return;
        }
        let skip = log.len().saturating_sub(rows);
        for (k, line) in log.iter().skip(skip).enumerate() {
            let ink = if crate::mole::is_section(line) {
                theme::TERMINAL_TEXT
            } else {
                p.secondary
            };
            self.text_colored(line, &self.fonts.mono_log, row(k), ink, 1.0);
        }
    }

    /// `.ck`: an outlined rounded square, or the accent one with a tick.
    fn paint_check(&self, r: Rect, on: bool, alpha: f32) {
        let p = self.palette;
        if !on {
            let inset = CHECK_BORDER / 2.0;
            let inner = Rect {
                x: r.x + inset,
                y: r.y + inset,
                w: r.w - CHECK_BORDER,
                h: r.h - CHECK_BORDER,
            };
            let target = self.target.as_ref().expect("target ensured by paint");
            unsafe {
                target.brush.SetColor(&color(p.secondary, alpha));
                target.rt.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: d2d_rect(inner),
                        radiusX: CHECK_RADIUS - inset,
                        radiusY: CHECK_RADIUS - inset,
                    },
                    &target.brush,
                    CHECK_BORDER,
                    None,
                );
            }
            return;
        }
        self.set_color(p.accent, alpha);
        self.fill_round(r, CHECK_RADIUS);
        // The design's 7 × 4 corner turned −45°, on the 16 px box.
        let k = r.w / CLEAN_CHECK;
        let at = |x: f32, y: f32| Vector2::new(r.x + x * k, r.y + y * k);
        let (a, b, c) = (at(4.4, 8.2), at(6.9, 10.6), at(11.6, 5.8));
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target.brush.SetColor(&color(p.on_accent, alpha));
            target
                .rt
                .DrawLine(a, b, &target.brush, CHECK_MARK, &self.round_cap);
            target
                .rt
                .DrawLine(b, c, &target.brush, CHECK_MARK, &self.round_cap);
        }
    }

    /// `.lock`: the ochre "admin" tag right after the row's name.
    fn paint_admin_tag(&self, name_area: Rect, name: &str, alpha: f32) {
        let p = self.palette;
        let lang = i18n::lang();
        let label = text(lang, Key::CleanAdminTag);
        let name_w = self.text_width(name, &self.fonts.row_name).ceil();
        let w = self.text_width(label, &self.fonts.micro).ceil() + 2.0 * ADMIN_PAD_X;
        let x = name_area.x + name_w + ADMIN_GAP;
        if x + w > name_area.right() {
            return;
        }
        let tag = Rect {
            x,
            y: name_area.y + (name_area.h - ADMIN_H) / 2.0,
            w,
            h: ADMIN_H,
        };
        self.set_color(p.ochre, ADMIN_TINT * alpha);
        self.fill_round(tag, tag.h / 2.0);
        self.text_colored(label, &self.fonts.micro_center, tag, p.ochre, alpha);
    }

    /// The measured header of the CLI page: each chip's full width and,
    /// while a command runs, the Stop button's.
    fn cli_head(&self, chips: &[CliChip], running: bool) -> CliHead {
        let lang = i18n::lang();
        let widths = chips
            .iter()
            .map(|c| {
                let w = self.text_width(&c.text, self.chip_format(c.look)).ceil();
                let mark = match c.look {
                    ChipLook::Run => SPIN + SPIN_GAP,
                    ChipLook::Explorer => CHIP_ICON + CHIP_ICON_GAP,
                    _ => 0.0,
                };
                w + mark + 2.0 * CHIP_PAD_X
            })
            .collect();
        let stop = running.then(|| {
            let w = self.text_width(text(lang, Key::CliStop), &self.fonts.small_medium);
            w.ceil() + STOP_SQUARE + STOP_GAP + 2.0 * STOP_PAD_X
        });
        CliHead {
            chips: widths,
            stop,
            explorer: chips.first().is_some_and(|c| c.look == ChipLook::Explorer),
            folder_menu: !running,
        }
    }

    fn chip_format(&self, look: ChipLook) -> &IDWriteTextFormat {
        match look {
            ChipLook::Mono => &self.fonts.mono_small,
            ChipLook::Path => &self.fonts.chip_mono,
            ChipLook::Run | ChipLook::Ok | ChipLook::Err | ChipLook::Explorer => &self.fonts.small,
        }
    }

    /// The CLI page as in pengingat-cli.html: title and chips, the terminal
    /// with output filling from the bottom, suggestions over the input, and
    /// the input line with its prompt, caret and hint.
    fn paint_cli(&self, l: &CliLayout, frame: &Frame) -> Result<()> {
        let lang = i18n::lang();
        let p = self.palette;
        let cli = &frame.cli;
        self.heading(text(lang, Key::Cli), l.header);
        let chips = cli_chips(lang, cli, frame.now);
        for (chip, r) in chips.iter().zip(&l.chips) {
            self.paint_cli_chip(chip, *r, cli.run, frame.now)?;
        }
        if let Some(stop) = l.stop {
            self.set_color(p.accent, 1.0);
            self.fill_round(stop, stop.h / 2.0);
            let mid = stop.y + stop.h / 2.0;
            let square = Rect {
                x: stop.x + STOP_PAD_X,
                y: mid - STOP_SQUARE / 2.0,
                w: STOP_SQUARE,
                h: STOP_SQUARE,
            };
            self.set_color(p.on_accent, 1.0);
            self.fill_round(square, 1.5);
            let label = Rect {
                x: square.right() + STOP_GAP,
                w: stop.right() - square.right() - STOP_GAP,
                ..stop
            };
            let word = text(lang, Key::CliStop);
            self.text_colored(word, &self.fonts.small_medium, label, p.on_accent, 1.0);
        }
        self.set_color(p.raised, 1.0);
        self.fill_round(l.expand, l.expand.w / 2.0);
        let icon = if cli.tall { Icon::Shrink } else { Icon::Expand };
        self.icon_colored(icon, l.expand, 13.0, p.secondary, 1.0, None);
        if cli.focused && frame.strip.is_none() {
            self.paint_cli_lock(l);
        }

        self.set_color(theme::TERMINAL_BG, 1.0);
        self.fill_round(l.term, CLI_TERM_RADIUS);
        self.paint_cli_output(l, cli);
        if cli.focused && !cli.suggestions.is_empty() {
            self.paint_cli_suggestions(l, cli);
        }
        self.paint_cli_input(l, cli)?;
        if let (Some(m), Some(menu)) = (&l.menu, cli.menu) {
            // The menu opens with a reveal of its own: the terminal behind
            // it keeps showing (`paint_panel_content` leaves this page out).
            // Not inside a page cross-fade: that already holds `layer2`
            // (`paint_page_at`), and one layer cannot be pushed twice.
            let fading = frame.transit.page_fade.and_then(page_fade_frame).is_some();
            match frame.transit.reveal.filter(|_| !fading) {
                Some(r) => {
                    let target = self.target.as_ref().expect("target ensured by paint");
                    let mut at = Matrix3x2::identity();
                    unsafe { target.rt.GetTransform(&mut at) };
                    self.push_opacity(r.alpha.clamp(0.0, 1.0));
                    self.set_translation(at.M31 + r.dx, at.M32 + r.dy);
                    self.paint_cli_menu(m, menu);
                    unsafe {
                        target.rt.SetTransform(&at);
                        target.rt.PopLayer();
                    }
                }
                None => self.paint_cli_menu(m, menu),
            }
        }
        Ok(())
    }

    /// The folder menu (keputusan-v3.1.html `.menu`): a raised box with a
    /// thin border; capital section headers; rows with an icon, the path
    /// (cut in the middle, keeping the last folder) and a small note; the
    /// last active Explorer window tinted; a star at the right of Explorer
    /// and recent rows, filled for a favourite.
    fn paint_cli_menu(&self, m: &MenuLayout, menu: &FolderMenuView) {
        let lang = i18n::lang();
        let p = self.palette;
        self.set_color(p.raised, 1.0);
        self.fill_round(m.boxed, MENU_RADIUS);
        self.stroke_round(inset(m.boxed, 0.5, 0.5), MENU_RADIUS, p.track);
        for (i, r) in &m.rows {
            let Some(item) = menu.items.get(*i) else {
                continue;
            };
            match item {
                MenuItem::Header(section) => {
                    let key = match section {
                        Section::Explorer => Key::CliMenuExplorer,
                        Section::Favorites => Key::CliMenuFavorites,
                        Section::Recent => Key::CliMenuRecent,
                    };
                    let label = text(lang, key).to_uppercase();
                    let area = Rect {
                        x: r.x + MENU_IN_X,
                        y: r.y + 2.0,
                        w: r.w - 2.0 * MENU_IN_X,
                        h: r.h - 2.0,
                    };
                    self.text2(&label, &self.fonts.micro, area);
                }
                MenuItem::Empty => {
                    let area = inset(*r, MENU_IN_X, 0.0);
                    let label = text(lang, Key::CliMenuEmpty);
                    self.text_colored(label, &self.fonts.micro, area, p.secondary, 0.8);
                }
                MenuItem::Folder { section, row } => {
                    if row.note == RowNote::LastActive {
                        self.set_color(p.accent, MENU_ON_TINT);
                        self.fill_round(*r, MENU_ROW_RADIUS);
                    }
                    let glyph = Rect {
                        x: r.x + MENU_IN_X,
                        w: MENU_ICON,
                        ..*r
                    };
                    match section {
                        Section::Explorer => self.icon_colored(
                            Icon::Folder,
                            glyph,
                            MENU_ICON,
                            p.secondary,
                            1.0,
                            None,
                        ),
                        Section::Favorites => self.icon_colored(
                            Icon::Star,
                            glyph,
                            MENU_ICON,
                            p.accent,
                            1.0,
                            Some(p.accent),
                        ),
                        Section::Recent => {
                            self.icon_colored(Icon::Clock, glyph, MENU_ICON, p.secondary, 1.0, None)
                        }
                    }
                    let mut right = r.right() - MENU_IN_X;
                    if let Some((_, star)) = m.stars.iter().find(|(k, _)| k == i) {
                        right = star.x;
                        let (ink, alpha, fill) = if row.favorite {
                            (p.accent, 1.0, Some(p.accent))
                        } else {
                            (p.secondary, 0.6, None)
                        };
                        self.icon_colored(Icon::Star, *star, MENU_STAR_ICON, ink, alpha, fill);
                    }
                    let note = match row.note {
                        RowNote::None => None,
                        RowNote::LastActive => Some(text(lang, Key::CliLastActive).to_owned()),
                        RowNote::Unpin => Some(text(lang, Key::CliUnpinHint).to_owned()),
                        RowNote::Ago(ago) => Some(i18n::folder_ago(lang, ago)),
                    };
                    if let Some(note) = note {
                        let w = self.text_width(&note, &self.fonts.micro).ceil() + 1.0;
                        let area = Rect {
                            x: right - w,
                            w,
                            ..*r
                        };
                        self.text2(&note, &self.fonts.micro, area);
                        right = area.x - MENU_GAP;
                    }
                    let x = glyph.right() + MENU_GAP;
                    let path = Rect {
                        x,
                        w: (right - x).max(0.0),
                        ..*r
                    };
                    let shown = row.path.to_string_lossy();
                    self.text_colored(&shown, &self.fonts.chip_mono, path, p.text, 1.0);
                }
            }
        }
    }

    fn paint_cli_chip(&self, chip: &CliChip, r: Rect, run: RunState, now: Instant) -> Result<()> {
        if r.w <= 0.0 {
            return Ok(());
        }
        let p = self.palette;
        let (fill, alpha, ink) = match chip.look {
            ChipLook::Explorer => (p.accent, 0.15, p.accent),
            ChipLook::Mono | ChipLook::Path => (p.raised, 1.0, p.secondary),
            ChipLook::Run => (p.accent, 0.15, p.accent),
            ChipLook::Ok => (p.sage, 0.15, p.sage),
            ChipLook::Err => (theme::SOFT_RED, 0.14, theme::SOFT_RED),
        };
        self.set_color(fill, alpha);
        self.fill_round(r, r.h / 2.0);
        let mut area = inset(r, CHIP_PAD_X, 0.0);
        if chip.look == ChipLook::Run {
            let turn = match run {
                RunState::Running { since } => now.saturating_duration_since(since).as_secs(),
                _ => 0,
            };
            let center = (area.x + SPIN / 2.0, r.y + r.h / 2.0);
            self.spinner(center, turn)?;
            area.x += SPIN + SPIN_GAP;
            area.w = (area.w - SPIN - SPIN_GAP).max(0.0);
        }
        if chip.look == ChipLook::Explorer {
            let glyph = Rect {
                w: CHIP_ICON,
                ..area
            };
            self.icon_colored(Icon::Folder, glyph, CHIP_ICON, p.accent, 1.0, None);
            area.x += CHIP_ICON + CHIP_ICON_GAP;
            area.w = (area.w - CHIP_ICON - CHIP_ICON_GAP).max(0.0);
        }
        self.text_colored(&chip.text, self.chip_format(chip.look), area, ink, 1.0);
        Ok(())
    }

    /// The design's `.spin`: a faint ring with a quarter in the accent,
    /// turned a quarter more each second the command runs.
    fn spinner(&self, (cx, cy): (f32, f32), turn: u64) -> Result<()> {
        let target = self.target.as_ref().expect("target ensured by paint");
        let r = SPIN / 2.0 - 1.0;
        let accent = self.palette.accent;
        unsafe {
            target.brush.SetColor(&color(accent, 0.25));
            let ellipse = D2D1_ELLIPSE {
                point: Vector2::new(cx, cy),
                radiusX: r,
                radiusY: r,
            };
            target.rt.DrawEllipse(&ellipse, &target.brush, 2.0, None);
            let start = -FRAC_PI_2 + (turn % 4) as f32 * FRAC_PI_2;
            let end = start + FRAC_PI_2;
            let path = self.d2d.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(
                Vector2::new(cx + r * start.cos(), cy + r * start.sin()),
                D2D1_FIGURE_BEGIN_HOLLOW,
            );
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: Vector2::new(cx + r * end.cos(), cy + r * end.sin()),
                size: D2D_SIZE_F {
                    width: r,
                    height: r,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            target.brush.SetColor(&color(accent, 1.0));
            target
                .rt
                .DrawGeometry(&path, &target.brush, 2.0, &self.round_cap);
        }
        Ok(())
    }

    /// "Panel tetap terbuka saat mengetik · Esc untuk keluar", hanging from
    /// the panel's top edge while the input has the keyboard.
    fn paint_cli_lock(&self, l: &CliLayout) {
        let p = self.palette;
        let label = text(i18n::lang(), Key::CliLocked);
        let tw = self.text_width(label, &self.fonts.micro).ceil();
        let w = LOCK_PAD_X * 2.0 + LOCK_DOT + LOCK_GAP + tw;
        let top = l.header.y - CLI_LOCK_ABOVE;
        let x = l.term.x + (l.term.w - w) / 2.0;
        // Rounded at the bottom only: the top corners rise past the edge,
        // where the panel's mask cuts them off.
        self.set_color(p.raised, 1.0);
        self.fill_round(
            Rect {
                x,
                y: top - LOCK_RADIUS,
                w,
                h: LOCK_H + LOCK_RADIUS,
            },
            LOCK_RADIUS,
        );
        let mid = top + LOCK_H / 2.0;
        let dot = Rect {
            x: x + LOCK_PAD_X,
            y: mid - LOCK_DOT / 2.0,
            w: LOCK_DOT,
            h: LOCK_DOT,
        };
        self.set_color(p.accent, 1.0);
        self.fill_round(dot, LOCK_DOT / 2.0);
        let area = Rect {
            x: dot.right() + LOCK_GAP,
            y: top,
            w: tw + 1.0,
            h: LOCK_H,
        };
        self.text2(label, &self.fonts.micro, area);
    }

    /// The output's last lines from the bottom up (`scroll` lines above the
    /// newest), coloured by kind; an empty terminal says where commands run.
    fn paint_cli_output(&self, l: &CliLayout, cli: &CliView) {
        let p = self.palette;
        let lines = cli.output.lines();
        let out = l.output;
        let row = |k: usize| Rect {
            x: out.x,
            y: out.bottom() - (k + 1) as f32 * CLI_LINE_H,
            w: out.w,
            h: CLI_LINE_H,
        };
        if lines.is_empty() {
            let lang = i18n::lang();
            let empty = [
                (text(lang, Key::CliEmpty), &self.fonts.mono),
                (cli.cwd, &self.fonts.mono),
                (text(lang, Key::CliEmptyHint), &self.fonts.small),
            ];
            for (k, (line, format)) in empty.iter().rev().enumerate() {
                self.text_colored(line, format, row(k), p.secondary, 0.8);
            }
            return;
        }
        let shown = visible_range(lines.len(), l.rows, cli.scroll);
        let first = shown.start;
        for (k, i) in shown.rev().enumerate() {
            let line = &lines[i];
            let r = row(k);
            let kind = line_kind(line, cli.echoes.is_echo(cli.output, i));
            match kind {
                LineKind::Command => {
                    if i > first {
                        // The design's `.sep` between one command's output
                        // and the next command.
                        self.set_color(theme::TERMINAL_SEP, 1.0);
                        self.fill_round(Rect { h: 1.0, ..r }, 0.0);
                    }
                    let mark = Rect { w: CLI_MARK_W, ..r };
                    self.text_colored("\u{203A}", &self.fonts.mono, mark, p.accent, 1.0);
                    let rest = Rect {
                        x: r.x + CLI_MARK_W,
                        w: r.w - CLI_MARK_W,
                        ..r
                    };
                    let cmd = echo_command(line);
                    self.text_colored(cmd, &self.fonts.mono, rest, theme::TERMINAL_TEXT, 1.0);
                }
                LineKind::Output => self.text_colored(line, &self.fonts.mono, r, p.secondary, 1.0),
                LineKind::Error => {
                    self.text_colored(line, &self.fonts.mono, r, theme::SOFT_RED, 1.0)
                }
                LineKind::Ok => self.text_colored(line, &self.fonts.mono, r, p.sage, 1.0),
                LineKind::Quiet => self.text_colored(line, &self.fonts.mono, r, p.secondary, 0.7),
            }
        }
    }

    /// Earlier commands matching the input in a raised box above it: the
    /// typed part in the accent, the highlighted row tinted.
    fn paint_cli_suggestions(&self, l: &CliLayout, cli: &CliView) {
        let p = self.palette;
        let (boxed, rows) = cli_suggestions(l, cli.suggestions.len());
        self.set_color(p.raised, 1.0);
        self.fill_round(boxed, SUGGEST_RADIUS);
        self.stroke_round(inset(boxed, 0.5, 0.5), SUGGEST_RADIUS, p.track);
        let typed = cli.input.trim_start();
        let selected = cli.selected.min(cli.suggestions.len().saturating_sub(1));
        for (i, (cmd, r)) in cli.suggestions.iter().zip(rows).enumerate() {
            if i == selected {
                self.set_color(p.accent, 0.16);
                self.fill_round(r, SUGGEST_ROW_RADIUS);
            }
            let area = inset(r, SUGGEST_PAD_X, 0.0);
            let head = &cmd[..typed.len().min(cmd.len())];
            let hw = self.text_width(head, &self.fonts.mono_medium);
            self.text_colored(head, &self.fonts.mono_medium, area, p.accent, 1.0);
            let rest = Rect {
                x: area.x + hw,
                w: (area.w - hw).max(0.0),
                ..area
            };
            let tail = &cmd[head.len()..];
            self.text_colored(tail, &self.fonts.mono, rest, theme::TERMINAL_TEXT, 1.0);
        }
    }

    /// The input line: the rule above it, the focus ring, `›`, the typed
    /// text with its selection, the rest of the highlighted suggestion in
    /// ghost ink, the caret, and the hint at the right.
    fn paint_cli_input(&self, l: &CliLayout, cli: &CliView) -> Result<()> {
        let lang = i18n::lang();
        let p = self.palette;
        let rule = Rect {
            y: l.input.y,
            h: 1.0,
            ..l.input
        };
        self.set_color(theme::TERMINAL_RULE, 1.0);
        self.fill_round(rule, 0.0);
        if cli.focused {
            self.focus_ring(l.input)?;
        }
        self.text_colored("\u{203A}", &self.fonts.mono, l.prompt, p.accent, 1.0);

        let running = cli.run.running();
        let hint = if running {
            Some(Key::CliHintRunning)
        } else if cli.focused && !cli.suggestions.is_empty() {
            Some(Key::CliHintSuggest)
        } else if cli.focused {
            Some(Key::CliHintReady)
        } else {
            None
        };
        let mut field = l.field;
        if let Some(key) = hint {
            let label = text(lang, key);
            let w = self.text_width(label, &self.fonts.micro).ceil() + 1.0;
            let area = Rect {
                x: field.right() - w,
                w,
                ..field
            };
            self.text_colored(label, &self.fonts.micro, area, theme::TERMINAL_HINT, 1.0);
            field.w = (area.x - HINT_GAP - field.x).max(0.0);
        }

        let mid = field.y + field.h / 2.0;
        if cli.input.is_empty() {
            let ghost = if running {
                Some(Key::CliWaiting)
            } else if !cli.focused {
                Some(Key::CliHintIdle)
            } else {
                None
            };
            if let Some(key) = ghost {
                let area = Rect {
                    x: field.x + if cli.focused { CARET_W + 4.0 } else { 0.0 },
                    ..field
                };
                let format = &self.fonts.mono;
                self.text_colored(text(lang, key), format, area, theme::TERMINAL_GHOST, 1.0);
            }
        }
        // Measured in the mono font: offsets are UTF-16, as the EDIT gives them.
        let len = cli.input.encode_utf16().count();
        let (a, b) = (cli.selection.0.min(len), cli.selection.1.min(len));
        let x_of = |n: usize| self.text_width(utf16_prefix(cli.input, n), &self.fonts.mono);
        let caret_x = x_of(b);
        // Long input scrolls left so the caret stays in view.
        let shift = (caret_x + CARET_W - field.w).max(0.0);
        let origin = field.x - shift;
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target
                .rt
                .PushAxisAlignedClip(&d2d_rect(field), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        if a != b && cli.focused {
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
        self.text_colored(
            cli.input,
            &self.fonts.mono,
            typed,
            theme::TERMINAL_TEXT,
            1.0,
        );
        let at_end = b == len;
        if cli.focused && at_end && !cli.input.is_empty() {
            let pick = cli.selected.min(cli.suggestions.len().saturating_sub(1));
            if let Some(rest) = cli
                .suggestions
                .get(pick)
                .and_then(|s| s.strip_prefix(cli.input))
            {
                let area = Rect {
                    x: origin + caret_x,
                    w: 4096.0,
                    ..field
                };
                self.text_colored(rest, &self.fonts.mono, area, theme::TERMINAL_GHOST, 1.0);
            }
        }
        if cli.focused {
            let caret = if at_end {
                Rect {
                    x: origin + caret_x,
                    y: mid - CARET_H / 2.0,
                    w: CARET_W,
                    h: CARET_H,
                }
            } else {
                Rect {
                    x: origin + caret_x - 0.75,
                    y: mid - CARET_H / 2.0,
                    w: 1.5,
                    h: CARET_H,
                }
            };
            self.set_color(p.accent, if at_end { 1.0 } else { 0.95 });
            self.fill_round(caret, 0.0);
            self.cli_caret.set(Some(caret));
        }
        unsafe { target.rt.PopAxisAlignedClip() };
        Ok(())
    }

    /// The design's `.inp.focus`: an inset accent line round the input,
    /// square at the top, following the terminal's bottom corners.
    fn focus_ring(&self, input: Rect) -> Result<()> {
        let target = self.target.as_ref().expect("target ensured by paint");
        let half = FOCUS_W / 2.0;
        let r = inset(input, half, half);
        let radius = CLI_TERM_RADIUS - half;
        let (left, right, top, bottom) = (r.x, r.right(), r.y, r.bottom());
        let corner = |x: f32, y: f32| D2D1_ARC_SEGMENT {
            point: Vector2::new(x, y),
            size: D2D_SIZE_F {
                width: radius,
                height: radius,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        };
        unsafe {
            let path = self.d2d.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(Vector2::new(left, top), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddLine(Vector2::new(right, top));
            sink.AddLine(Vector2::new(right, bottom - radius));
            sink.AddArc(&corner(right - radius, bottom));
            sink.AddLine(Vector2::new(left + radius, bottom));
            sink.AddArc(&corner(left, bottom - radius));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            target.brush.SetColor(&color(self.palette.accent, 0.55));
            target.rt.DrawGeometry(&path, &target.brush, FOCUS_W, None);
        }
        Ok(())
    }

    /// The Clipboard page after keputusan-v3.1.html §5: the title, the
    /// search field, and the cards (kind label with its icon, the value on
    /// two lines, a swatch for a colour, the thumbnail strip for an image),
    /// the selected one ringed in the accent with its buttons.
    fn paint_clipboard(&self, l: &ClipLayout, clip: &ClipView) -> Result<()> {
        let lang = i18n::lang();
        let p = self.palette;
        self.heading(text(lang, Key::ClipboardTitle), l.header);
        self.paint_clip_search(l.search, clip);

        if let Some(area) = l.empty.or((!clip.enabled).then_some(l.list)) {
            let (title, hint) = if !clip.enabled {
                (Key::ClipOff, Some(Key::ClipOffHint))
            } else if clip.total == 0 {
                (Key::ClipEmpty, Some(Key::ClipEmptyHint))
            } else {
                (Key::ClipNoMatch, None)
            };
            let mid = area.y + area.h / 2.0 - 12.0;
            let line = |y: f32| Rect { y, h: 18.0, ..area };
            self.text(text(lang, title), &self.fonts.body_center, line(mid), 1.0);
            if let Some(hint) = hint {
                self.text_colored(
                    text(lang, hint),
                    &self.fonts.small_center,
                    line(mid + 20.0),
                    p.secondary,
                    1.0,
                );
            }
            if !clip.enabled {
                return Ok(());
            }
        }

        if let Some(bar) = l.scrollbar {
            self.set_color(p.secondary, 0.55);
            self.fill_round(bar, CLIP_SCROLLBAR_W / 2.0);
        }

        let mut drawn_images = Vec::new();
        for &(id, _, r) in &l.cards {
            let Some(item) = clip.items.iter().find(|c| c.id == id) else {
                continue;
            };
            self.set_color(p.raised, 1.0);
            self.fill_round(r, CLIP_RADIUS);
            let selected = clip.selected == Some(id);
            match item.image() {
                Some(img) => {
                    drawn_images.push(id);
                    self.paint_clip_image(r, item, img, clip.now, lang)?;
                }
                None => self.paint_clip_text(r, item, lang),
            }
            if item.pinned && !selected {
                let pin = Rect {
                    x: r.right() - 8.0 - 12.0,
                    y: r.y + 7.0,
                    w: 12.0,
                    h: 12.0,
                };
                self.icon_colored(Icon::Pin, pin, 12.0, p.accent, 1.0, None);
            }
            if selected {
                self.stroke_round_width(inset(r, 0.75, 0.75), CLIP_RADIUS - 0.75, p.accent, 1.5);
            }
        }
        self.clip_thumbs
            .borrow_mut()
            .retain(|(id, _)| drawn_images.contains(id));

        for &(button, id, r) in &l.buttons {
            let pinned = clip.items.iter().any(|c| c.id == id && c.pinned);
            self.set_color(p.raised_hi, 1.0);
            self.fill_round(r, r.h / 2.0);
            let (icon, ink) = match button {
                ClipButton::Pin if pinned => (Icon::Pin, p.accent),
                ClipButton::Pin => (Icon::Pin, p.text),
                ClipButton::Plain => (Icon::Text, p.text),
                ClipButton::Delete => (Icon::Trash, p.text),
            };
            self.icon_colored(icon, r, 12.0, ink, 1.0, None);
        }
        Ok(())
    }

    /// `.search`: the raised field, the glyph, the placeholder or the typed
    /// text with its selection and caret, and the accent ring while focused.
    fn paint_clip_search(&self, r: Rect, clip: &ClipView) {
        let hint = i18n::clip_search_in(i18n::lang(), clip.total);
        let caret = self.paint_text_field(r, clip.query, clip.selection, clip.focused, &hint);
        if caret.is_some() {
            self.clip_caret.set(caret);
        }
    }

    /// The quick search: its field, then a row per result (icon, name, what
    /// it is), the highlighted one ringed in the accent. With nothing
    /// typed, a line of what to try. A row shows the shell's icon once the
    /// details worker has it (its glyph until then), and adds a folder's
    /// count or an app's size to its subtitle.
    fn paint_search(&self, l: &SearchLayout, view: &SearchView) {
        let lang = i18n::lang();
        let p = self.palette;
        let hint = text(lang, Key::SearchHint);
        // The back button: the same raised round shape as the field, the
        // arrow in the icons' ink.
        self.set_color(p.raised, 1.0);
        self.fill_round(l.back, 12.0);
        self.icon_colored(Icon::Back, l.back, 16.0, p.secondary, 1.0, None);
        let caret = self.paint_text_field(l.field, view.query, view.selection, view.focused, hint);
        if caret.is_some() {
            self.search_caret.set(caret);
        }
        if let Some(area) = l.empty {
            let mid = area.y + area.h / 2.0 - 12.0;
            let line = |y: f32| Rect { y, h: 18.0, ..area };
            let typed = view.query.trim().chars().count();
            if view.indexing && typed >= crate::search::FILE_MIN_CHARS {
                // The first walk of the user's folders is still running and
                // nothing else matched: say so, quietly.
                self.text_colored(
                    text(lang, Key::SearchIndexing),
                    &self.fonts.small_center,
                    line(mid + 10.0),
                    p.secondary,
                    1.0,
                );
            } else {
                let (title, sub) = if typed == 0 {
                    (Key::SearchEmpty, Key::SearchEmptyHint)
                } else {
                    (Key::SearchNone, Key::SearchNoneHint)
                };
                self.text(text(lang, title), &self.fonts.body_center, line(mid), 1.0);
                self.text_colored(
                    text(lang, sub),
                    &self.fonts.small_center,
                    line(mid + 20.0),
                    p.secondary,
                    1.0,
                );
            }
        }
        // Icons of rows no longer shown go.
        self.search_icons.borrow_mut().retain(|(path, _)| {
            view.results
                .iter()
                .any(|r| r.path.as_deref() == Some(path.as_path()))
        });
        for (i, (row, res)) in l.rows.iter().zip(view.results).enumerate() {
            let on = i == view.selected;
            if on {
                self.set_color(p.raised, 1.0);
                self.fill_round(*row, 10.0);
                self.stroke_round_width(inset(*row, 0.75, 0.75), 9.25, p.accent, 1.5);
            }
            let icon = Rect {
                x: row.x + 10.0,
                w: 16.0,
                ..*row
            };
            let glyph = match res.kind {
                SearchKind::App => Icon::Grid,
                SearchKind::Setting => Icon::Gear,
                SearchKind::Calc => Icon::Calc,
                SearchKind::Web => Icon::Globe,
                SearchKind::Folder => Icon::Folder,
                SearchKind::File => Icon::Note,
            };
            let ink = if on { p.accent } else { p.secondary };
            let detail = res.path.as_ref().and_then(|path| view.details.get(path));
            let shell_icon = res
                .path
                .as_deref()
                .zip(detail.and_then(|d| d.icon.as_ref()))
                .and_then(|(path, thumb)| self.search_icon(path, thumb));
            match shell_icon {
                Some(bitmap) => {
                    let side = 16.0;
                    let at = Rect {
                        x: icon.x,
                        y: row.y + (row.h - side) / 2.0,
                        w: side,
                        h: side,
                    };
                    let target = self.target.as_ref().expect("target ensured by paint");
                    unsafe {
                        target.rt.DrawBitmap(
                            &bitmap,
                            Some(&d2d_rect(at)),
                            1.0,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            None,
                        );
                    }
                }
                None => {
                    let glyph = match (res.kind, res.path.as_deref()) {
                        (SearchKind::File, Some(path)) => {
                            crate::shelf::icon(crate::shelf::kind_of(path, false))
                        }
                        _ => glyph,
                    };
                    self.icon_colored(glyph, icon, 16.0, ink, 1.0, None);
                }
            }
            // What it is (or where, for a file), small and quiet at the
            // right, with a folder's count or an app's size once known; the
            // name takes the rest.
            let note = detail
                .and_then(|d| crate::search_details::row_note(res.kind, d, lang))
                .map(|note| format!("{} \u{b7} {note}", res.subtitle));
            let subtitle = note.as_deref().unwrap_or(&res.subtitle);
            let sub_w = self.text_width(subtitle, &self.fonts.small).ceil() + 2.0;
            let sub_w = sub_w.min(row.w * 0.4);
            let sub = Rect {
                x: row.right() - 10.0 - sub_w,
                w: sub_w,
                ..*row
            };
            self.text_colored(subtitle, &self.fonts.small, sub, p.secondary, 1.0);
            let name = Rect {
                x: icon.right() + 10.0,
                w: (sub.x - 10.0 - icon.right() - 10.0).max(0.0),
                ..*row
            };
            self.text(&res.title, &self.fonts.body_medium, name, 1.0);
        }
    }

    /// The shell icon of `path` as a bitmap of this target, made once. A
    /// failure (or a lost target, which the frame's end reports) shows the
    /// glyph instead.
    fn search_icon(&self, path: &std::path::Path, thumb: &Thumb) -> Option<ID2D1Bitmap> {
        if let Some((_, b)) = self.search_icons.borrow().iter().find(|(p, _)| p == path) {
            return Some(b.clone());
        }
        let bitmap = self.thumb_bitmap(thumb).ok().flatten()?;
        self.search_icons
            .borrow_mut()
            .push((path.to_path_buf(), bitmap.clone()));
        Some(bitmap)
    }

    /// A rounded text field drawn like the Clipboard's: the hint while it is
    /// empty and unfocused, the text, a selection and a caret while it has
    /// the keyboard. Returns the caret it drew (for the IME), if any.
    fn paint_text_field(
        &self,
        r: Rect,
        query: &str,
        selection: (usize, usize),
        focused: bool,
        hint: &str,
    ) -> Option<Rect> {
        let p = self.palette;
        self.set_color(p.raised, 1.0);
        self.fill_round(r, 12.0);
        if focused {
            self.stroke_round_width(inset(r, 0.75, 0.75), 11.25, p.accent, FOCUS_W);
        }
        let glyph = Rect {
            x: r.x + 11.0,
            w: 14.0,
            ..r
        };
        self.icon_colored(Icon::Search, glyph, 14.0, p.secondary, 1.0, None);
        let field = Rect {
            x: glyph.right() + 8.0,
            w: (r.right() - 11.0 - glyph.right() - 8.0).max(0.0),
            ..r
        };
        let format = &self.fonts.clip_search;
        let mid = field.y + field.h / 2.0;
        if query.is_empty() && !focused {
            self.text_colored(hint, format, field, p.secondary, 1.0);
            return None;
        }
        let len = query.encode_utf16().count();
        let (a, b) = (selection.0.min(len), selection.1.min(len));
        let x_of = |n: usize| self.text_width(utf16_prefix(query, n), format);
        let caret_x = x_of(b);
        let shift = (caret_x + 2.0 - field.w).max(0.0);
        let origin = field.x - shift;
        let mut drawn = None;
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target
                .rt
                .PushAxisAlignedClip(&d2d_rect(field), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        if query.is_empty() {
            self.text_colored(hint, format, field, p.secondary, 0.6);
        }
        if a != b && focused {
            let xa = x_of(a);
            self.set_color(p.accent, 0.32);
            let sel = Rect {
                x: origin + xa,
                y: mid - CARET_H / 2.0,
                w: caret_x - xa,
                h: CARET_H,
            };
            self.fill_round(sel, 2.0);
        }
        let typed = Rect {
            x: origin,
            w: 4096.0,
            ..field
        };
        self.text(query, format, typed, 1.0);
        if focused {
            let caret = Rect {
                x: origin + caret_x - 0.75,
                y: mid - CARET_H / 2.0,
                w: 1.5,
                h: CARET_H,
            };
            self.set_color(p.accent, 1.0);
            self.fill_round(caret, 0.0);
            drawn = Some(caret);
        }
        unsafe { target.rt.PopAxisAlignedClip() };
        drawn
    }

    /// The kind label (`.k`: icon and uppercase name) at `at`.
    fn paint_clip_kind(&self, at: Rect, kind: ClipKind, lang: Lang) {
        let p = self.palette;
        let glyph = Rect { w: 11.0, ..at };
        self.icon_colored(clip_icon(kind), glyph, 11.0, p.secondary, 1.0, None);
        let label = Rect {
            x: glyph.right() + 5.0,
            w: (at.right() - glyph.right() - 5.0).max(0.0),
            ..at
        };
        let name = text(lang, i18n::clip_kind_key(kind)).to_uppercase();
        self.text_colored(&name, &self.fonts.clip_kind, label, p.secondary, 1.0);
    }

    fn paint_clip_text(&self, r: Rect, item: &Clip, lang: Lang) {
        let p = self.palette;
        let inner = inset(r, 10.0, 8.0);
        // Room for the pin or the selected card's buttons at the top right.
        let kind_row = Rect {
            h: 12.0,
            w: (inner.w - 70.0).max(0.0),
            ..inner
        };
        self.paint_clip_kind(kind_row, item.kind, lang);
        let mut value = Rect {
            y: kind_row.bottom() + 3.0,
            h: (inner.bottom() - kind_row.bottom() - 3.0).max(0.0),
            ..inner
        };
        let shown = card_value(item);
        if item.kind == ClipKind::Color
            && let Some(rgb) = hex_rgb(&shown)
        {
            let swatch = Rect {
                x: value.x,
                y: value.y,
                w: 18.0,
                h: 18.0,
            };
            self.set_color(rgb, 1.0);
            self.fill_round(swatch, 6.0);
            value = Rect {
                x: swatch.right() + 6.0,
                w: (value.right() - swatch.right() - 6.0).max(0.0),
                h: 18.0,
                ..value
            };
            self.text_colored(&shown, &self.fonts.eleven, value, p.text, 1.0);
            return;
        }
        let format = if item.kind.mono() {
            &self.fonts.clip_mono
        } else {
            &self.fonts.clip_value
        };
        self.text_colored(&shown, format, value, p.text, 1.0);
    }

    /// `.clip.img`: the thumbnail filling the card's left 92 px (its left
    /// corners rounded with the card), then the label, size and age.
    fn paint_clip_image(
        &self,
        r: Rect,
        item: &Clip,
        img: &ClipImage,
        now: NaiveDateTime,
        lang: Lang,
    ) -> Result<()> {
        let p = self.palette;
        let strip = Rect {
            w: CLIP_THUMB_W,
            ..r
        };
        let target = self.target.as_ref().expect("target ensured by paint");
        if let Some(bitmap) = self.clip_thumb(item.id, &img.thumb)? {
            unsafe {
                let D2D_SIZE_F { width, height } = bitmap.GetSize();
                let k = (strip.w / width.max(1.0)).max(strip.h / height.max(1.0));
                let transform = Matrix3x2 {
                    M11: k,
                    M12: 0.0,
                    M21: 0.0,
                    M22: k,
                    M31: strip.x + (strip.w - width * k) / 2.0,
                    M32: strip.y + (strip.h - height * k) / 2.0,
                };
                let brush = target.rt.CreateBitmapBrush(&bitmap, None, None)?;
                brush.SetTransform(&transform);
                let brush: ID2D1Brush = brush.cast()?;
                target
                    .rt
                    .PushAxisAlignedClip(&d2d_rect(strip), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                target.rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: d2d_rect(r),
                        radiusX: CLIP_RADIUS,
                        radiusY: CLIP_RADIUS,
                    },
                    &brush,
                );
                target.rt.PopAxisAlignedClip();
            }
        }
        let meta = inset(
            Rect {
                x: strip.right(),
                w: r.right() - strip.right(),
                ..r
            },
            10.0,
            8.0,
        );
        let kind_row = Rect {
            h: 12.0,
            w: (meta.w - 48.0).max(0.0),
            ..meta
        };
        self.paint_clip_kind(kind_row, ClipKind::Image, lang);
        let size = i18n::clip_image_meta(lang, img.screenshot, img.width, img.height);
        let value = Rect {
            y: kind_row.bottom() + 3.0,
            h: 16.0,
            ..meta
        };
        self.text_colored(&size, &self.fonts.eleven, value, p.text, 1.0);
        let when = crate::cli_folders::local_time(item.at)
            .map(|then| i18n::folder_ago(lang, crate::cli_folders::ago(then, now)))
            .unwrap_or_default();
        let age = Rect {
            y: value.bottom() + 2.0,
            h: 13.0,
            ..meta
        };
        self.text_colored(&when, &self.fonts.micro, age, p.secondary, 1.0);
        Ok(())
    }

    /// The thumbnail of entry `id` as a bitmap of this target, made once.
    pub(super) fn clip_thumb(&self, id: u64, thumb: &Thumb) -> Result<Option<ID2D1Bitmap>> {
        if let Some((_, b)) = self.clip_thumbs.borrow().iter().find(|(i, _)| *i == id) {
            return Ok(Some(b.clone()));
        }
        let Some(bitmap) = self.thumb_bitmap(thumb)? else {
            return Ok(None);
        };
        self.clip_thumbs.borrow_mut().push((id, bitmap.clone()));
        Ok(Some(bitmap))
    }

    /// The logo `thumb` of logo key `key` as a bitmap of this target, made
    /// once.
    pub(super) fn notif_logo(&self, key: &str, thumb: &Thumb) -> Result<Option<ID2D1Bitmap>> {
        if let Some((_, b)) = self.notif_logos.borrow().iter().find(|(k, _)| k == key) {
            return Ok(Some(b.clone()));
        }
        let Some(bitmap) = self.thumb_bitmap(thumb)? else {
            return Ok(None);
        };
        self.notif_logos
            .borrow_mut()
            .push((key.to_owned(), bitmap.clone()));
        Ok(Some(bitmap))
    }

    /// The shelf: a card per file (its type icon over its name, an X on the
    /// corner), "Kosongkan" beside the heading. A card whose file is gone is
    /// dimmed.
    /// The shelf: its title with the count and a quiet "Kosongkan", cards
    /// with an icon over a name of up to two lines, and a dashed zone that
    /// lights in the accent while files are dragged over the island.
    fn paint_shelf(&self, l: &ShelfLayout, frame: &Frame) {
        let lang = i18n::lang();
        let p = self.palette;
        let (cards, dragging) = (frame.shelf, frame.shelf_drag);
        let (motion, now) = (frame.shelf_motion, frame.now);
        // Cards on their way out still count, and keep the empty hint away,
        // until the last of them is gone.
        let leaving = motion.ghost_count(now);
        let shown = cards.len() + leaving;
        let title = match shown {
            0 => text(lang, Key::PageShelf).to_owned(),
            n => format!("{} \u{b7} {n}", text(lang, Key::PageShelf)),
        };
        self.heading(&title, l.header);
        if let Some(clear) = l.clear {
            self.set_color(p.raised, 1.0);
            self.fill_round(clear, clear.h / 2.0);
            self.text_colored(
                text(lang, Key::ShelfClear),
                &self.fonts.small_center,
                clear,
                p.secondary,
                1.0,
            );
        }
        if dragging {
            self.set_color(p.accent, 0.08);
            self.fill_round(l.zone, radius::TILE);
        }
        if let Some(empty) = l.empty.filter(|_| leaving == 0) {
            if !dragging {
                self.stroke_dashed(empty, radius::TILE, p.track, 1.5);
            }
            let (icon_area, text_area) = (
                Rect {
                    y: empty.y + empty.h / 2.0 - 34.0,
                    h: 30.0,
                    ..empty
                },
                Rect {
                    y: empty.y + empty.h / 2.0 + 2.0,
                    h: 20.0,
                    ..empty
                },
            );
            let ink = if dragging { p.accent } else { p.secondary };
            self.icon_colored(Icon::Download, icon_area, 26.0, ink, 1.0, None);
            let hint = text(
                lang,
                if dragging {
                    Key::ShelfDropHere
                } else {
                    Key::ShelfEmpty
                },
            );
            self.text_colored(hint, &self.fonts.eleven_center, text_area, ink, 1.0);
        }
        // The cards that left, under the ones that stay. Per-draw alpha and
        // a transform only: no layer, so this is safe inside a page
        // cross-fade or reveal (they hold `layer2`).
        for (card, drawn) in motion.ghosts(now) {
            self.paint_shelf_card(card, drawn);
        }
        for (lay, card) in l.cards.iter().zip(cards) {
            let drawn = motion.card_at(&card.key, lay.card, l.lines, now);
            self.paint_shelf_card(card, drawn);
        }
        // The outline last, so cards never cut it.
        if dragging {
            self.stroke_dashed(l.zone, radius::TILE, p.accent, 1.5);
        }
    }

    /// One card as `drawn` says: its parts laid out in the rect it is at with
    /// its name on the lines it has, all at its opacity, scaled around its
    /// centre and shifted down by `dy`.
    fn paint_shelf_card(&self, card: &ShelfCard, drawn: Drawn) {
        if drawn.alpha <= 0.0 {
            return;
        }
        let lang = i18n::lang();
        let p = self.palette;
        let lines = drawn.lines;
        let lay = shelf_card_layout(drawn.rect, lines);
        let target = self.target.as_ref().expect("target ensured by paint");
        let mut at = Matrix3x2::identity();
        let moved = !drawn.at_rest();
        if moved {
            // Whatever shifts the page (a slide, a reveal) is a translation:
            // the scale is composed under it.
            unsafe { target.rt.GetTransform(&mut at) };
            let (cx, cy) = (lay.card.x + lay.card.w / 2.0, lay.card.y + lay.card.h / 2.0);
            let s = drawn.scale;
            let scaled = Matrix3x2 {
                M11: s,
                M12: 0.0,
                M21: 0.0,
                M22: s,
                M31: at.M31 + cx * (1.0 - s),
                M32: at.M32 + cy * (1.0 - s) + drawn.dy,
            };
            unsafe { target.rt.SetTransform(&scaled) };
        }
        let a = drawn.alpha;
        let ink = if card.stale { 0.4 * a } else { a };
        // As long as a name fits the card: about 6.4 px a character.
        let per_line = (lay.label.w / 6.4) as usize;
        self.set_color(p.raised, a);
        self.fill_round(lay.card, radius::TILE);
        self.icon(crate::shelf::icon(card.kind), lay.glyph, 26.0, ink, true);
        let names = crate::shelf::wrap_name(&card.name, per_line.max(6), lines);
        for (i, line) in names.iter().enumerate() {
            let row = Rect {
                y: lay.label.y + i as f32 * (lay.label.h / lines as f32),
                h: lay.label.h / lines as f32,
                ..lay.label
            };
            self.text(line, &self.fonts.eleven_center, row, ink);
        }
        // The X on a small round chip, so it reads on any card.
        self.set_color(p.raised_hi, 0.9 * a);
        self.fill_round(lay.remove, lay.remove.h / 2.0);
        self.icon_colored(Icon::X, lay.remove, 10.0, p.secondary, a, None);
        if card.stale {
            self.set_color(p.track, a);
            self.fill_round(lay.missing, lay.missing.h / 2.0);
            self.text(
                text(lang, Key::ShelfMissing),
                &self.fonts.micro_center,
                lay.missing,
                a,
            );
        }
        if moved {
            unsafe { target.rt.SetTransform(&at) };
        }
    }

    /// A dashed outline of `r` (the shelf's drop zone).
    fn stroke_dashed(&self, r: Rect, radius: f32, rgb: u32, width: f32) {
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target.brush.SetColor(&color(rgb, 1.0));
            target.rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: d2d_rect(inset(r, width / 2.0, width / 2.0)),
                    radiusX: radius,
                    radiusY: radius,
                },
                &target.brush,
                width,
                Some(&self.dashed),
            );
        }
    }

    /// The Mixer: a speaker, a name, a slider and the percent per row; the
    /// master level first. A muted row is dimmed with the crossed speaker.
    fn paint_mixer(&self, l: &MixerLayout, rows: Option<&[MixerRow]>) {
        let lang = i18n::lang();
        let p = self.palette;
        self.heading(text(lang, Key::PageMixer), l.header);
        let Some(rows) = rows else {
            self.text2(
                text(lang, Key::MixerUnavailable),
                &self.fonts.eleven,
                l.rows[0].name,
            );
            return;
        };
        for (row, lay) in rows.iter().zip(&l.rows) {
            let dim = if row.muted { 0.45 } else { 1.0 };
            let icon = if row.muted {
                Icon::VolumeMute
            } else {
                Icon::Volume
            };
            let ink = if row.muted { p.secondary } else { p.text };
            self.icon_colored(icon, lay.icon, 18.0, ink, 1.0, None);
            let name = match row.target {
                Target::Master => text(lang, Key::MixerMaster),
                Target::System => text(lang, Key::MixerSystemSounds),
                Target::App(_) => row.name.as_str(),
            };
            let name_area = Rect {
                w: lay.name.w,
                ..lay.name
            };
            self.text_colored(name, &self.fonts.body_medium, name_area, p.text, dim);
            // The track, its filled part and a knob at the level.
            let bar = Rect {
                y: lay.track.y + (lay.track.h - MIXER_TRACK_H) / 2.0,
                h: MIXER_TRACK_H,
                ..lay.track
            };
            self.set_color(p.track, 1.0);
            self.fill_round(bar, bar.h / 2.0);
            let fill = Rect {
                w: bar.w * row.level,
                ..bar
            };
            self.set_color(p.accent, dim);
            if fill.w > 0.0 {
                self.fill_round(fill, bar.h / 2.0);
            }
            let knob = 14.0;
            self.set_color(p.text, dim);
            self.fill_round(
                Rect {
                    x: (fill.right() - knob / 2.0).clamp(bar.x, bar.right() - knob),
                    y: bar.y + (bar.h - knob) / 2.0,
                    w: knob,
                    h: knob,
                },
                knob / 2.0,
            );
            let value = format!("{}%", audio::percent(row.level));
            self.text_colored(
                &value,
                &self.fonts.track_time_right,
                lay.value,
                p.secondary,
                dim,
            );
        }
        if rows.len() < 2 {
            self.text2(
                text(lang, Key::MixerEmpty),
                &self.fonts.eleven,
                l.rows[1].name,
            );
        }
    }

    /// Rows of tiles: a duotone icon over its label.
    fn paint_shortcuts(&self, l: &ShortcutsLayout, tiles: &[Tile]) {
        let lang = i18n::lang();
        let heading = text(lang, Key::Shortcuts);
        self.heading(heading, l.header);
        for ((_, rect), tile) in l.tiles.iter().zip(tiles) {
            self.set_color(self.palette.raised, 1.0);
            self.fill_round(*rect, radius::TILE);
            let (glyph, label) = tile_parts(*rect);
            let (icon, name) = match tile {
                Tile::Builtin(s) => (shortcuts::icon(*s), text(lang, shortcuts::label(*s))),
                Tile::Custom(c) => (shortcuts::custom_icon(c.kind), c.label.as_str()),
            };
            self.icon(icon, glyph, 24.0, 1.0, true);
            self.text(name, &self.fonts.eleven_center, label, 1.0);
        }
    }

    fn paint_calendar(&mut self, l: &CalendarLayout, frame: &Frame) {
        let lang = i18n::lang();
        let p = self.palette;
        let snap = frame.calendar;
        let events = &snap.events;
        let title = calendar_title(lang, l.view, frame.cal_anchor, l.selected);
        self.heading(&title, l.header);
        self.icon_colored(Icon::ChevL, l.prev, 18.0, p.secondary, 1.0, None);
        self.icon_colored(Icon::ChevR, l.next, 18.0, p.secondary, 1.0, None);
        if let Some(tag) = l.offline {
            self.set_color(p.track, 1.0);
            self.fill_round(tag, tag.h / 2.0);
            self.text2(text(lang, Key::Offline), &self.fonts.micro_center, tag);
        }

        // View selector: one raised track, the active segment lit.
        let first = l.view_buttons[0].1;
        let last = l.view_buttons[2].1;
        let track = Rect {
            x: first.x - SEG_PAD,
            w: last.right() - first.x + 2.0 * SEG_PAD,
            ..first
        };
        self.set_color(p.raised, 1.0);
        self.fill_round(track, radius::CONTROL);
        for (view, r) in l.view_buttons {
            let on = view == l.view;
            if on {
                self.set_color(p.raised_hi, 1.0);
                self.fill_round(inset(r, 0.0, SEG_PAD), radius::SEGMENT);
            }
            let label = text(
                lang,
                match view {
                    CalView::Day => Key::ViewDay,
                    CalView::Week => Key::ViewWeek,
                    CalView::Month => Key::ViewMonth,
                },
            );
            let ink = if on { p.text } else { p.secondary };
            self.text_colored(label, &self.fonts.small_center, r, ink, 1.0);
        }

        match l.view {
            CalView::Month => self.paint_month(l, frame, lang),
            CalView::Week => self.paint_week(l, frame, lang),
            CalView::Day => self.paint_day(l, events, lang),
        }
        self.paint_add(l, snap.status, lang);
    }

    fn paint_month(&self, l: &CalendarLayout, frame: &Frame, lang: Lang) {
        let p = self.palette;
        let events = &frame.calendar.events;
        // The grid starts on a Monday: the first row names the weekdays.
        for ((d, _), r) in l.month_cells.iter().zip(&l.weekday_labels) {
            self.text2(i18n::day_short(lang, *d), &self.fonts.micro_center, *r);
        }
        for (d, r) in &l.month_cells {
            self.paint_day_cell(*d, *r, frame.today, l.selected, frame.cal_anchor, events);
        }
        // The line between the grid and the list, midway.
        if let Some((_, last)) = l.month_cells.get(6) {
            let x = (last.right() + l.list.x) / 2.0;
            self.set_color(p.track, 1.0);
            self.fill_round(
                Rect {
                    x: x - 0.5,
                    y: l.list.y,
                    w: 1.0,
                    h: l.list.h,
                },
                0.0,
            );
        }
        if let Some(caption) = l.caption {
            let day = i18n::day_title_long(lang, l.selected);
            self.text2(&day, &self.fonts.small, caption);
        }
        for (id, r) in &l.rows {
            let Some(ev) = events.iter().find(|e| &e.id == id) else {
                continue;
            };
            self.set_color(event_color(ev, &p), 1.0);
            self.fill_round(
                Rect {
                    x: r.x,
                    y: r.y + 2.0,
                    w: 3.0,
                    h: r.h - 10.0,
                },
                1.5,
            );
            let text_x = r.x + 12.0;
            let w = r.right() - text_x;
            let title = Rect {
                x: text_x,
                y: r.y,
                w,
                h: 17.0,
            };
            let time = Rect {
                x: text_x,
                y: r.y + 16.0,
                w,
                h: 14.0,
            };
            self.text(ev.shown_title(), &self.fonts.body_medium, title, 1.0);
            self.text2(&event_time_label(lang, ev), &self.fonts.small, time);
        }
        if let Some((n, r)) = l.more {
            self.text2(&more_label(lang, n), &self.fonts.small, r);
        }
    }

    /// A month grid day (`month_grid::paint_day`): today in an accent
    /// circle, the selected day ringed, days of other months dim, a sage dot
    /// under other days with events.
    fn paint_day_cell(
        &self,
        d: NaiveDate,
        r: Rect,
        today: NaiveDate,
        selected: NaiveDate,
        anchor: NaiveDate,
        events: &[CalEvent],
    ) {
        let p = self.palette;
        let look = CellLook {
            filled: Filled::Today,
            ring_w: 1.0,
            ring_ink_accent: false,
            accent: p.accent,
            on_accent: p.on_accent,
            text: p.text,
            dim: p.dot_off,
            dot: p.sage,
            hover: None,
        };
        let state = DayState {
            events: !events_on(events, d).is_empty(),
            ..DayState::of(d, today, selected, anchor)
        };
        paint_day(self, d, day_geometry(r, 3.0, 26.0), state, &look);
    }

    /// Seven columns: the day's name and number (today in the accent), then
    /// its events as chips with a coloured edge; all-day ones as tinted bands.
    fn paint_week(&self, l: &CalendarLayout, frame: &Frame, lang: Lang) {
        let p = self.palette;
        let events = &frame.calendar.events;
        for (d, col) in &l.week_columns {
            self.set_color(p.column, 1.0);
            self.fill_round(*col, radius::COLUMN);
            let name = Rect {
                y: col.y + 6.0,
                h: 12.0,
                ..*col
            };
            let number = Rect {
                y: col.y + 17.0,
                h: 18.0,
                ..*col
            };
            self.text2(i18n::day_short(lang, *d), &self.fonts.micro_center, name);
            let label = d.day().to_string();
            let ink = if *d == frame.today { p.accent } else { p.text };
            self.text_colored(&label, &self.fonts.day_number, number, ink, 1.0);
            if *d == l.selected && *d != frame.today {
                let head = Rect {
                    h: WEEK_HEAD_H,
                    ..*col
                };
                self.stroke_round(inset(head, 3.0, 3.0), radius::SEGMENT, p.accent);
            }
        }
        for (id, r) in &l.rows {
            let Some(ev) = events.iter().find(|e| &e.id == id) else {
                continue;
            };
            let edge = event_color(ev, &p);
            if ev.all_day() {
                self.set_color(edge, CHIP_TINT);
            } else {
                self.set_color(p.raised, 1.0);
            }
            self.fill_round(*r, radius::EVENT_CHIP);
            self.set_color(edge, 1.0);
            self.fill_round(
                Rect {
                    x: r.x + 1.0,
                    y: r.y + 3.0,
                    w: 2.5,
                    h: r.h - 6.0,
                },
                1.25,
            );
            let inner = Rect {
                x: r.x + 7.0,
                w: r.w - 9.0,
                ..*r
            };
            if ev.all_day() {
                self.text(ev.shown_title(), &self.fonts.chip_title, inner, 1.0);
            } else {
                let title = Rect {
                    y: r.y + 3.0,
                    h: 13.0,
                    ..inner
                };
                let time = Rect {
                    y: r.y + 16.0,
                    h: 11.0,
                    ..inner
                };
                self.text(ev.shown_title(), &self.fonts.chip_title, title, 1.0);
                if let EventTime::At(start) = &ev.start {
                    let at = i18n::time_hm(lang, start.naive_local());
                    self.text2(&at, &self.fonts.chip_time, time);
                }
            }
        }
        for (_, n, r) in &l.week_more {
            self.text2(&format!("+{n}"), &self.fonts.micro_center, *r);
        }
    }

    /// All-day bands, then the hour lines with their labels, the events as
    /// tinted blocks, and the "now" line with its dot.
    fn paint_day(&self, l: &CalendarLayout, events: &[CalEvent], lang: Lang) {
        let p = self.palette;
        for (hour, y) in &l.hours {
            let label = Rect {
                x: l.list.x,
                y: y - 6.0,
                w: HOUR_GUTTER - 6.0,
                h: 12.0,
            };
            self.text2(&hour_label(lang, *hour), &self.fonts.micro, label);
            self.set_color(p.track, 1.0);
            self.fill_round(
                Rect {
                    x: l.list.x + HOUR_GUTTER - 4.0,
                    y: y - 0.5,
                    w: l.list.right() - (l.list.x + HOUR_GUTTER - 4.0),
                    h: 1.0,
                },
                0.0,
            );
        }
        for (id, r) in &l.rows {
            let Some(ev) = events.iter().find(|e| &e.id == id) else {
                continue;
            };
            let base = event_color(ev, &p);
            let ink = mix(base, p.text, BLOCK_TEXT_MIX);
            if ev.all_day() {
                self.set_color(base, BAND_TINT);
                self.fill_round(*r, radius::SEGMENT);
                let label = format!("{} · {}", text(lang, Key::AllDay), ev.shown_title());
                let area = inset(*r, 9.0, 0.0);
                self.text_colored(&label, &self.fonts.small, area, ink, 1.0);
                continue;
            }
            self.set_color(base, BLOCK_TINT);
            self.fill_round(*r, radius::BLOCK);
            let inner = inset(*r, 9.0, 0.0);
            let time = event_time_label(lang, ev);
            if r.h >= 30.0 {
                let title = Rect {
                    y: r.y + 3.0,
                    h: 15.0,
                    ..inner
                };
                let at = Rect {
                    y: r.y + 17.0,
                    h: 12.0,
                    ..inner
                };
                self.text_colored(ev.shown_title(), &self.fonts.eleven, title, ink, 1.0);
                self.text_colored(&time, &self.fonts.micro, at, ink, 0.8);
            } else {
                let line = match &ev.start {
                    EventTime::At(t) => {
                        format!(
                            "{} · {}",
                            ev.shown_title(),
                            i18n::time_hm(lang, t.naive_local())
                        )
                    }
                    EventTime::AllDay(_) => ev.shown_title().to_owned(),
                };
                self.text_colored(&line, &self.fonts.eleven, inner, ink, 1.0);
            }
        }
        if let Some(y) = l.now_y {
            let x = l.list.x + HOUR_GUTTER - 6.0;
            self.set_color(p.accent, 1.0);
            self.fill_round(
                Rect {
                    x,
                    y: y - 0.75,
                    w: l.list.right() - x,
                    h: 1.5,
                },
                0.75,
            );
            self.fill_round(
                Rect {
                    x: x - 3.5,
                    y: y - 3.5,
                    w: 7.0,
                    h: 7.0,
                },
                3.5,
            );
        }
        if let Some((n, r)) = l.more {
            self.text2(&more_label(lang, n), &self.fonts.small, r);
        }
    }

    /// "+ Tambah acara" in the accent, and the Google button.
    fn paint_add(&self, l: &CalendarLayout, status: CalStatus, lang: Lang) {
        let p = self.palette;
        let plus = Rect { w: 16.0, ..l.add };
        self.icon_colored(Icon::Plus, plus, 13.0, p.accent, 1.0, None);
        let label = Rect {
            x: l.add.x + 20.0,
            w: l.add.w - 20.0,
            ..l.add
        };
        let add = text(lang, Key::AddEvent);
        self.text_colored(add, &self.fonts.eleven, label, p.accent, 1.0);
        if let Some(r) = l.connect {
            self.set_color(p.accent, 1.0);
            self.fill_round(r, radius::CONTROL);
            let label = text(
                lang,
                if status == CalStatus::Reconnect {
                    Key::ReconnectGoogle
                } else {
                    Key::ConnectGoogle
                },
            );
            self.text_colored(label, &self.fonts.body_center, r, p.on_accent, 1.0);
        }
    }

    /// The big ring with the countdown and the phase; beside it the session
    /// with its dots, reset / the primary start-pause / skip, and what comes
    /// next.
    fn paint_pomodoro(&mut self, l: &PomodoroLayout, frame: &Frame) -> Result<()> {
        let lang = i18n::lang();
        let p = self.palette;
        let pomo = frame.pomodoro;
        let settings = pomo.settings();
        let status = pomo.status();
        let tint = p.phase(pomo.phase());
        self.heading(text(lang, Key::Pomodoro), l.header);
        let durations = i18n::pomo_durations(
            lang,
            settings.focus_min,
            settings.short_break_min,
            settings.long_break_min,
        );
        let right = &self.fonts.body_right;
        self.text_colored(&durations, right, l.durations, p.secondary, 1.0);

        self.ring(
            l.ring_center,
            l.ring_radius,
            RING_STROKE,
            pomo.progress(frame.now),
            tint,
        )?;
        let paused = matches!(status, PomoStatus::Paused { .. });
        let time = mmss(pomo.remaining(frame.now));
        let alpha = if paused { 0.6 } else { 1.0 };
        self.text(&time, &self.fonts.big_time, l.time, alpha);
        let phase = phase_label(lang, pomo).to_uppercase();
        self.text_colored(&phase, &self.fonts.small_center_bold, l.phase, tint, 1.0);

        // "Sesi 2 dari 4" and a dot per session, filled up to this one.
        let n = settings.sessions;
        let k = pomo.session().min(n);
        let session = i18n::session_of(lang, k, n);
        self.text2(&session, &self.fonts.secondary, l.session);
        let dot = 8.0;
        let mut x = l.session.x + self.text_width(&session, &self.fonts.secondary) + 8.0;
        for i in 0..n {
            if x + dot > l.session.right() {
                break;
            }
            let r = Rect {
                x,
                y: l.session.y + (l.session.h - dot) / 2.0,
                w: dot,
                h: dot,
            };
            if i < k {
                self.set_color(tint, 1.0);
                self.fill_round(r, dot / 2.0);
            } else {
                self.stroke_round_width(inset(r, 0.75, 0.75), dot / 2.0, p.dot_off, 1.5);
            }
            x += dot + 5.0;
        }

        for (r, icon) in [(l.reset, Icon::Reset), (l.skip, Icon::Skip)] {
            self.set_color(p.raised, 1.0);
            self.fill_round(r, r.w / 2.0);
            self.icon(icon, r, 18.0, 1.0, false);
        }
        self.set_color(tint, 1.0);
        self.fill_round(l.start_pause, l.start_pause.w / 2.0);
        let main = primary_icon(status);
        self.icon_colored(main, l.start_pause, 24.0, p.on_accent, 1.0, None);

        // "Berikutnya: Istirahat 5 menit", the phase in its colour.
        let (next, minutes) = next_phase(pomo);
        let lead = text(lang, Key::NextUp);
        self.text2(lead, &self.fonts.eleven, l.next_up);
        let after = self.text_width(lead, &self.fonts.eleven) + 4.0;
        let what = i18n::phase_minutes(lang, phase_name(lang, next), minutes);
        let rest = Rect {
            x: l.next_up.x + after,
            w: (l.next_up.w - after).max(0.0),
            ..l.next_up
        };
        let ink = p.phase(next);
        self.text_colored(&what, &self.fonts.button, rest, ink, 1.0);
        Ok(())
    }

    /// "+ Baru": a raised chip with a plus.
    fn paint_new_button(&self, r: Rect, lang: Lang) {
        let label = text(lang, Key::New);
        let w = (self.text_width(label, &self.fonts.button) + 38.0).min(r.w);
        let chip = Rect {
            x: r.right() - w,
            w,
            ..r
        };
        self.set_color(self.palette.raised, 1.0);
        self.fill_round(chip, chip.h / 2.0);
        let plus = Rect {
            x: chip.x + 9.0,
            w: 14.0,
            ..chip
        };
        self.icon(Icon::Plus, plus, 13.0, 1.0, false);
        let area = Rect {
            x: plus.right() + 4.0,
            w: chip.right() - plus.right() - 4.0,
            ..chip
        };
        self.text(label, &self.fonts.button, area, 1.0);
    }

    /// The hint under the header of an empty list.
    fn paint_empty_hint(&self, l: &ListLayout, key: Key, lang: Lang) {
        let hint = Rect {
            y: l.header.bottom() + 8.0,
            h: 20.0,
            ..l.header
        };
        self.text2(text(lang, key), &self.fonts.secondary, hint);
    }

    /// Cards in two columns: the first line as the title, the rest as a
    /// snippet, the time at the foot.
    fn paint_notes(&self, l: &ListLayout, notes: &[Note], today: NaiveDate) {
        let lang = i18n::lang();
        let p = self.palette;
        self.heading(text(lang, Key::Notes), l.header);
        self.paint_new_button(l.new_button, lang);
        if notes.is_empty() {
            self.paint_empty_hint(l, Key::NotesEmpty, lang);
        }
        for (id, r) in &l.rows {
            let Some(note) = notes.iter().find(|n| &n.id == id) else {
                continue;
            };
            self.set_color(p.raised, 1.0);
            self.fill_round(*r, radius::CARD);
            let inner = inset(*r, 13.0, 0.0);
            let title = Rect {
                y: r.y + 9.0,
                h: 18.0,
                ..inner
            };
            self.text(first_line(&note.text), &self.fonts.body_medium, title, 1.0);
            let foot = Rect {
                y: r.bottom() - 22.0,
                h: 14.0,
                ..inner
            };
            let snippet = Rect {
                y: title.bottom() + 2.0,
                h: (foot.y - title.bottom() - 4.0).max(0.0),
                ..inner
            };
            self.text2(&note_snippet(&note.text), &self.fonts.snippet, snippet);
            let when = DateTime::from_timestamp(note.updated, 0)
                .map(|t| note_time_label(lang, t.with_timezone(&Local).naive_local(), today))
                .unwrap_or_default();
            let ink = p.secondary;
            self.text_colored(&when, &self.fonts.micro, foot, ink, NOTE_TIME_ALPHA);
        }
        if let Some((n, r)) = l.more {
            self.text2(&more_label(lang, n), &self.fonts.small, r);
        }
    }

    /// Day groups ("HARI INI", "BESOK", a date) of rows: a ring, the time,
    /// the text, an ochre "Terlewat" tag when overdue, a repeat icon when it
    /// repeats; a hairline under each row.
    fn paint_reminders(&self, l: &ListLayout, reminders: &[Reminder], today: NaiveDate) {
        let lang = i18n::lang();
        let p = self.palette;
        let now = Local::now().timestamp();
        self.heading(text(lang, Key::Reminders), l.header);
        self.paint_new_button(l.new_button, lang);
        if reminders.is_empty() {
            self.paint_empty_hint(l, Key::RemindersEmpty, lang);
        }
        for (day, r) in &l.groups {
            let label = day_label(lang, *day, today).to_uppercase();
            self.text2(&label, &self.fonts.micro, *r);
        }
        for (id, r) in &l.rows {
            let Some(reminder) = reminders.iter().find(|x| &x.id == id) else {
                continue;
            };
            self.set_color(p.column, 1.0);
            self.fill_round(
                Rect {
                    y: r.bottom() - 1.0,
                    h: 1.0,
                    ..*r
                },
                0.0,
            );
            let ring = Rect { w: 20.0, ..*r };
            self.icon_colored(Icon::Ring, ring, 17.0, p.secondary, 1.0, None);
            let (time, line) = reminder_row(lang, reminder);
            let at = Rect {
                x: r.x + 30.0,
                w: 40.0,
                ..*r
            };
            self.text2(&time, &self.fonts.eleven, at);
            // From the right: the repeat icon, then the tag.
            let mut end = r.right();
            if reminder.repeat != Repeat::Once {
                let icon = Rect {
                    x: end - 16.0,
                    w: 16.0,
                    ..*r
                };
                self.icon_colored(Icon::Repeat, icon, 15.0, p.secondary, 1.0, None);
                end = icon.x - 8.0;
            }
            if reminder.due_unix <= now {
                let label = text(lang, Key::Missed);
                let w = self.text_width(label, &self.fonts.micro) + 14.0;
                let tag = Rect {
                    x: end - w,
                    y: r.y + (r.h - 17.0) / 2.0,
                    w,
                    h: 17.0,
                };
                self.set_color(p.ochre, TAG_TINT);
                self.fill_round(tag, tag.h / 2.0);
                self.text_colored(label, &self.fonts.micro_center, tag, p.ochre, 1.0);
                end = tag.x - 8.0;
            }
            let body = Rect {
                x: at.right() + 8.0,
                w: (end - at.right() - 8.0).max(0.0),
                ..*r
            };
            self.text(line, &self.fonts.secondary, body, 1.0);
        }
        if let Some((n, r)) = l.more {
            self.text2(&more_label(lang, n), &self.fonts.small, r);
        }
    }

    /// The big cover; "SEDANG DIPUTAR", the title in DM Sans 600, the artist;
    /// the progress bar with its times; prev, the round play/pause, next,
    /// and "Lirik".
    fn paint_media(
        &mut self,
        l: &ExpandedLayout,
        media: Option<&MediaInfo>,
        lyrics_on: bool,
        progress: Option<Progress>,
        volume: Option<f32>,
    ) -> Result<()> {
        let p = self.palette;
        self.paint_art(l.art, radius::COVER, media.and_then(|m| m.art.as_deref()))?;
        if let Some(level) = volume {
            // Over the cover's lower part, where the wheel works.
            let strip = Rect {
                y: l.art.bottom() - VOLUME_HUD_H - 8.0,
                h: VOLUME_HUD_H,
                x: l.art.x + 8.0,
                w: l.art.w - 16.0,
            };
            self.paint_volume(strip, level, 0.72);
        }
        self.paint_lyrics_button(l.lyrics, lyrics_on);
        let Some(media) = media else {
            self.text(
                i18n::tr(Key::NothingPlaying),
                &self.fonts.artist,
                l.title,
                0.8,
            );
            self.text2(
                i18n::tr(Key::NothingPlayingHint),
                &self.fonts.small,
                l.artist,
            );
            return Ok(());
        };
        let label = i18n::tr(Key::NowPlaying).to_uppercase();
        self.text2(&label, &self.fonts.micro, l.label);
        self.text(&media.title, &self.fonts.song, l.title, 1.0);
        self.text2(&media.artist, &self.fonts.artist, l.artist);
        if let Some(progress) = progress {
            self.paint_progress(l, progress);
        }
        let play = if media.playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        let [prev, middle, next] = l.buttons;
        self.icon(Icon::Prev, prev, 20.0, 1.0, false);
        self.set_color(p.accent, 1.0);
        self.fill_round(middle, middle.w / 2.0);
        self.icon_colored(play, middle, 22.0, p.on_accent, 1.0, None);
        self.icon(Icon::Next, next, 20.0, 1.0, false);
        Ok(())
    }

    /// The volume while the wheel turns it: a dark rounded plate over `area`
    /// (`plate`: how opaque) with the speaker, the level as a bar and the
    /// percentage.
    pub(super) fn paint_volume(&self, area: Rect, level: f32, plate: f32) {
        let p = self.palette;
        let l = volume_hud_layout(area);
        self.set_color(0x000000, plate);
        self.fill_round(area, area.h / 2.0);
        let icon = if level <= 0.0 {
            Icon::VolumeMute
        } else {
            Icon::Volume
        };
        self.icon_colored(icon, l.icon, 16.0, 0xFFFFFF, 1.0, None);
        self.set_color(0xFFFFFF, 0.25);
        self.fill_round(l.track, l.track.h / 2.0);
        if level > 0.0 {
            let fill = Rect {
                w: (l.track.w * level.clamp(0.0, 1.0)).max(l.track.h),
                ..l.track
            };
            self.set_color(p.accent, 1.0);
            self.fill_round(fill, fill.h / 2.0);
        }
        let value = format!("{}%", crate::audio::percent(level));
        self.text_colored(&value, &self.fonts.small_center, l.value, 0xFFFFFF, 1.0);
    }

    /// The track and its played part in the accent, a knob at the playhead,
    /// and the elapsed and total times under the ends.
    fn paint_progress(&self, l: &ExpandedLayout, progress: Progress) {
        let Some(fraction) = progress.fraction() else {
            return;
        };
        let p = self.palette;
        let m = media_progress_layout(l);
        let bar = m.bar;
        self.set_color(p.track, 1.0);
        self.fill_round(bar, bar.h / 2.0);
        let fill = Rect {
            w: bar.w * fraction,
            ..bar
        };
        self.set_color(p.accent, 1.0);
        if fill.w > 0.0 {
            self.fill_round(fill, bar.h / 2.0);
        }
        let knob = Rect {
            x: fill.right() - PROGRESS_KNOB / 2.0,
            y: bar.y + (bar.h - PROGRESS_KNOB) / 2.0,
            w: PROGRESS_KNOB,
            h: PROGRESS_KNOB,
        };
        self.fill_round(knob, PROGRESS_KNOB / 2.0);
        let elapsed = track_time(progress.position_ms.min(progress.end_ms));
        self.text2(&elapsed, &self.fonts.track_time, m.elapsed);
        let total = track_time(progress.end_ms);
        self.text2(&total, &self.fonts.track_time_right, m.total);
    }

    /// "Lirik" with a quote mark: outlined in the accent while lyrics show in
    /// the notch, quiet otherwise.
    fn paint_lyrics_button(&self, r: Rect, on: bool) {
        let p = self.palette;
        let ink = if on { p.accent } else { p.secondary };
        if on {
            self.set_color(p.accent, TAG_TINT);
            self.fill_round(r, r.h / 2.0);
        }
        self.stroke_round(
            inset(r, 0.5, 0.5),
            r.h / 2.0,
            if on { p.accent } else { p.dot_off },
        );
        let label = i18n::tr(Key::LyricsButton);
        let w = 14.0 + 5.0 + self.text_width(label, &self.fonts.button);
        let x = r.x + (r.w - w) / 2.0;
        let icon = Rect { x, w: 14.0, ..r };
        self.icon_colored(Icon::Quote, icon, 12.0, ink, 1.0, None);
        let area = Rect {
            x: icon.right() + 5.0,
            w: (r.right() - icon.right() - 5.0).max(0.0),
            ..r
        };
        self.text_colored(label, &self.fonts.button, area, ink, 1.0);
    }

    /// Orange dot and "Claude butuh respon · {project}", or an accent dot and
    /// "{reminder} · 09.30".
    fn paint_strip(
        &self,
        s: ShapeRect,
        notice: &Notice,
        badge: Option<ToastBadgeView>,
        alpha: f32,
    ) -> Result<()> {
        let faded = alpha < 1.0;
        if faded {
            self.push_opacity(alpha.clamp(0.0, 0.999));
        }
        let painted = self.paint_strip_inner(s, notice, badge);
        if faded {
            let target = self.target.as_ref().expect("target ensured by paint");
            unsafe { target.rt.PopLayer() };
        }
        painted
    }

    fn paint_strip_inner(
        &self,
        s: ShapeRect,
        notice: &Notice,
        badge: Option<ToastBadgeView>,
    ) -> Result<()> {
        let r = waiting_strip(s);
        self.set_color(self.palette.raised, 1.0);
        self.fill_round(r, radius::CONTROL);
        // A busy toast: its 16 dip logo (or letter tile), then
        // "Source · title" (the body when there is no title).
        if crate::state::is_real_toast(notice) {
            let (logo, area) = toast_strip_parts(r);
            let badge = badge.unwrap_or(ToastBadgeView {
                source: &notice.source,
                logo: None,
            });
            self.paint_toast_badge(logo, badge)?;
            let said = if notice.title.is_empty() {
                &notice.body
            } else {
                &notice.title
            };
            let text = match (notice.source.is_empty(), said.is_empty()) {
                (_, true) => notice.source.clone(),
                (true, false) => said.clone(),
                (false, false) => format!("{} · {said}", notice.source),
            };
            self.text(&text, &self.fonts.secondary, area, 1.0);
            return Ok(());
        }
        let dot = match notice.kind {
            NoticeKind::Reminder { .. } => self.palette.accent,
            _ => CLAUDE,
        };
        self.set_color(dot, 1.0);
        self.fill_round(
            Rect {
                x: r.x + 10.0,
                y: r.y + r.h / 2.0 - 4.0,
                w: 8.0,
                h: 8.0,
            },
            4.0,
        );
        let text = if notice.body.is_empty() {
            notice.title.clone()
        } else {
            format!("{} · {}", notice.title, notice.body)
        };
        let area = Rect {
            x: r.x + 26.0,
            w: r.w - 36.0,
            ..r
        };
        self.text(&text, &self.fonts.secondary, area, 1.0);
        Ok(())
    }

    /// Small ring, `mm:ss`, then "Fokus 2/4", "Istirahat" or "Siap · klik".
    pub(super) fn paint_pomodoro_compact(&mut self, shape: ShapeRect, frame: &Frame) -> Result<()> {
        let pomo = frame.pomodoro;
        let (center, radius, text) = match frame.robot {
            Some(robot) => {
                let (slot, center, radius, text) = pomodoro_compact_layout_robot(shape);
                self.paint_robot(slot, robot);
                (center, radius, text)
            }
            None => pomodoro_compact_layout(shape),
        };
        let tint = self.palette.phase(pomo.phase());
        self.ring(center, radius, 3.0, pomo.progress(frame.now), tint)?;
        let paused = matches!(pomo.status(), PomoStatus::Paused { .. });
        let time = Rect { w: 42.0, ..text };
        let label = Rect {
            x: time.right() + 4.0,
            w: (text.right() - time.right() - 4.0).max(0.0),
            ..text
        };
        self.text(
            &mmss(pomo.remaining(frame.now)),
            &self.fonts.title,
            time,
            if paused { 0.6 } else { 1.0 },
        );
        let lang = i18n::lang();
        self.text2(&compact_label(lang, pomo), &self.fonts.eleven, label);
        Ok(())
    }

    /// Track plus a clockwise arc from 12 o'clock covering `progress`.
    fn ring(
        &self,
        (cx, cy): (f32, f32),
        r: f32,
        stroke: f32,
        progress: f32,
        rgb: u32,
    ) -> Result<()> {
        let target = self.target.as_ref().expect("target ensured by paint");
        let ellipse = D2D1_ELLIPSE {
            point: Vector2::new(cx, cy),
            radiusX: r,
            radiusY: r,
        };
        unsafe {
            target.brush.SetColor(&color(self.palette.track, 1.0));
            target.rt.DrawEllipse(&ellipse, &target.brush, stroke, None);
        }
        let p = progress.clamp(0.0, 1.0);
        if p <= 0.0 {
            return Ok(());
        }
        unsafe {
            target.brush.SetColor(&color(rgb, 1.0));
            if p >= 0.999 {
                target.rt.DrawEllipse(&ellipse, &target.brush, stroke, None);
                return Ok(());
            }
            let angle = -FRAC_PI_2 + TAU * p;
            let path = self.d2d.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(Vector2::new(cx, cy - r), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: Vector2::new(cx + r * angle.cos(), cy + r * angle.sin()),
                size: D2D_SIZE_F {
                    width: r,
                    height: r,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if p > 0.5 {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            target
                .rt
                .DrawGeometry(&path, &target.brush, stroke, &self.round_cap);
        }
        Ok(())
    }

    /// Everything drawn until `PopLayer` is clipped to `mask` at `opacity`.
    fn push_mask(&self, mask: &ID2D1Geometry, opacity: f32) {
        let target = self.target.as_ref().expect("target ensured by paint");
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F {
                left: -f32::MAX,
                top: -f32::MAX,
                right: f32::MAX,
                bottom: f32::MAX,
            },
            geometricMask: ManuallyDrop::new(Some(mask.clone())),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        unsafe { target.rt.PushLayer(&params, &target.layer) };
        // The layer keeps its own reference while pushed.
        drop(ManuallyDrop::into_inner(params.geometricMask));
    }

    /// Shifts everything drawn next `dx` logical px to the right.
    fn set_offset(&self, dx: f32) {
        self.set_translation(dx, 0.0);
    }

    /// Shifts everything drawn next `dx` right and `dy` down.
    fn set_translation(&self, dx: f32, dy: f32) {
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe { target.rt.SetTransform(&Matrix3x2::translation(dx, dy)) };
    }

    /// A 1 px outline.
    fn stroke_round(&self, r: Rect, radius: f32, rgb: u32) {
        self.stroke_round_width(r, radius, rgb, 1.0);
    }

    fn stroke_round_width(&self, r: Rect, radius: f32, rgb: u32, width: f32) {
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target.brush.SetColor(&color(rgb, 1.0));
            target.rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: d2d_rect(r),
                    radiusX: radius,
                    radiusY: radius,
                },
                &target.brush,
                width,
                None,
            );
        }
    }
}

impl CellCanvas for Renderer {
    fn disc(&self, circle: Rect, rgb: u32) {
        self.set_color(rgb, 1.0);
        self.fill_round(circle, circle.w / 2.0);
    }

    fn ring(&self, circle: Rect, width: f32, rgb: u32) {
        let half = width / 2.0;
        self.stroke_round_width(inset(circle, half, half), circle.w / 2.0, rgb, width);
    }

    fn number(&self, text: &str, area: Rect, rgb: u32, bold: bool) {
        let format = if bold {
            &self.fonts.cell_bold
        } else {
            &self.fonts.cell
        };
        self.text_colored(text, format, area, rgb, 1.0);
    }
}

/// The calendar's title: the month, the week's range or the day.
fn calendar_title(lang: Lang, view: CalView, anchor: NaiveDate, selected: NaiveDate) -> String {
    match view {
        CalView::Month => i18n::month_title(lang, anchor),
        CalView::Week => i18n::week_title(lang, range_for(CalView::Week, anchor).0),
        CalView::Day => i18n::day_title_long(lang, selected),
    }
}

/// An event's own colour, else sage for all-day events and the accent for
/// timed ones.
fn event_color(ev: &CalEvent, p: &Palette) -> u32 {
    ev.color
        .unwrap_or(if ev.all_day() { p.sage } else { p.accent })
}

/// A Clipboard kind's icon on its card.
fn clip_icon(kind: ClipKind) -> Icon {
    match kind {
        ClipKind::Text => Icon::Text,
        ClipKind::Link => Icon::Link,
        ClipKind::Code => Icon::Code,
        ClipKind::Color => Icon::Swatch,
        ClipKind::Image => Icon::Image,
        ClipKind::Files => Icon::Folder,
    }
}

/// What a text or files card shows: the text (its start: two lines is all
/// that fits), or the files' names.
fn card_value(item: &Clip) -> String {
    match &item.data {
        ClipData::Text(t) => {
            let t = t.trim();
            match t.char_indices().nth(400) {
                Some((i, _)) => t[..i].to_owned(),
                None => t.to_owned(),
            }
        }
        ClipData::Files(paths) => paths
            .iter()
            .map(|p| {
                p.file_name()
                    .unwrap_or(p.as_os_str())
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>()
            .join(", "),
        ClipData::Image(_) => String::new(),
    }
}

fn inset(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect {
        x: r.x + dx,
        y: r.y + dy,
        w: (r.w - 2.0 * dx).max(0.0),
        h: (r.h - 2.0 * dy).max(0.0),
    }
}

fn phase_name(lang: Lang, phase: Phase) -> &'static str {
    text(
        lang,
        match phase {
            Phase::Focus => Key::Focus,
            Phase::ShortBreak => Key::Break,
            Phase::LongBreak => Key::LongBreak,
        },
    )
}

/// The phase after the current one and its minutes: a break after focus
/// (the long one after the last session), focus after a break.
fn next_phase(pomo: &Pomodoro) -> (Phase, u32) {
    let s = pomo.settings();
    match pomo.phase() {
        Phase::Focus if pomo.session() >= s.sessions => (Phase::LongBreak, s.long_break_min),
        Phase::Focus => (Phase::ShortBreak, s.short_break_min),
        Phase::ShortBreak | Phase::LongBreak => (Phase::Focus, s.focus_min),
    }
}

/// "+N lagi", or "Kembali ke atas" on the last chunk (`n` = 0).
fn more_label(lang: Lang, n: usize) -> String {
    if n == 0 {
        text(lang, Key::BackToTop).into()
    } else {
        i18n::more_count(lang, n)
    }
}

/// "09.00" (Indonesian) or "09:00": an hour line's label; 24 for the end of
/// the day.
fn hour_label(lang: Lang, hour: u32) -> String {
    let sep = match lang {
        Lang::Id => '.',
        Lang::En => ':',
    };
    format!("{hour:02}{sep}00")
}

/// "10.00 – 11.30", "Seharian", "Seharian · 24 Okt – 26 Okt", or both dates
/// for a timed event spanning days.
fn event_time_label(lang: Lang, ev: &CalEvent) -> String {
    let date = |d| i18n::short_date(lang, d);
    let hm = |t| i18n::time_hm(lang, t);
    match (&ev.start, &ev.end) {
        (EventTime::AllDay(s), EventTime::AllDay(e)) => {
            let all_day = text(lang, Key::AllDay);
            let last = e.pred_opt().unwrap_or(*s);
            if last <= *s {
                all_day.into()
            } else {
                format!("{all_day} · {} – {}", date(*s), date(last))
            }
        }
        (EventTime::At(s), EventTime::At(e)) => {
            let (s, e) = (s.naive_local(), e.naive_local());
            if s.date() == e.date() {
                format!("{} – {}", hm(s), hm(e))
            } else {
                format!(
                    "{} {} – {} {}",
                    date(s.date()),
                    hm(s),
                    date(e.date()),
                    hm(e)
                )
            }
        }
        _ => String::new(),
    }
}

/// "hari ini 09.12", "kemarin", "23 Okt", or "23 Okt 2025" in another year.
fn note_time_label(lang: Lang, updated: NaiveDateTime, today: NaiveDate) -> String {
    let d = updated.date();
    if d == today {
        format!(
            "{} {}",
            text(lang, Key::Today),
            i18n::time_hm(lang, updated)
        )
    } else if Some(d) == today.pred_opt() {
        text(lang, Key::Yesterday).into()
    } else if d.year() == today.year() {
        i18n::short_date(lang, d)
    } else {
        format!("{} {}", i18n::short_date(lang, d), d.year())
    }
}

/// A reminder row's time ("09.30") and text (its first line).
fn reminder_row(lang: Lang, r: &Reminder) -> (String, &str) {
    let time = local_time(r.due_unix)
        .map(|due| i18n::time_hm(lang, due))
        .unwrap_or_default();
    (time, first_line(&r.text))
}

/// A day group's heading: "Hari ini", "Besok", "Sel, 27 Okt", or
/// "3 Jan 2027" in another year.
fn day_label(lang: Lang, d: NaiveDate, today: NaiveDate) -> String {
    if d == today {
        text(lang, Key::DayToday).into()
    } else if Some(d) == today.succ_opt() {
        text(lang, Key::DayTomorrow).into()
    } else if d.year() == today.year() {
        format!(
            "{}, {}",
            i18n::day_short(lang, d),
            i18n::short_date(lang, d)
        )
    } else {
        format!("{} {}", i18n::short_date(lang, d), d.year())
    }
}

/// The first non-blank line, trimmed.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// A note card's snippet: the non-blank lines after the first, joined.
fn note_snippet(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whole seconds rounded up, so a fresh 25-minute phase shows "25:00".
fn mmss(d: Duration) -> String {
    let secs = d.as_millis().div_ceil(1000);
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

fn compact_label(lang: Lang, pomo: &Pomodoro) -> String {
    if pomo.status() == PomoStatus::Ready {
        return text(lang, Key::ReadyClick).into();
    }
    match pomo.phase() {
        Phase::Focus => {
            let n = pomo.settings().sessions;
            let focus = text(lang, Key::Focus);
            format!("{focus} {}/{n}", pomo.session().min(n))
        }
        Phase::ShortBreak | Phase::LongBreak => text(lang, Key::Break).into(),
    }
}

/// Under the countdown: the phase, marked when paused or waiting.
fn phase_label(lang: Lang, pomo: &Pomodoro) -> String {
    let name = phase_name(lang, pomo.phase());
    match pomo.status() {
        PomoStatus::Paused { .. } => format!("{name} · {}", text(lang, Key::Paused)),
        PomoStatus::Ready => format!("{name} {}", text(lang, Key::Ready)),
        PomoStatus::Idle | PomoStatus::Running { .. } => name.into(),
    }
}

/// The primary button's icon: pause while running; start, resume or start
/// the next phase otherwise.
fn primary_icon(status: PomoStatus) -> Icon {
    match status {
        PomoStatus::Running { .. } => Icon::Pause,
        PomoStatus::Idle | PomoStatus::Ready | PomoStatus::Paused { .. } => Icon::Play,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clean::{CatSize, ScanResult};
    use crate::config::UiFont;
    use crate::pomodoro::PomoSettings;
    use crate::ui::render::{MONO_BIG_TIME, MONO_CLOCK};
    use crate::theme::{Accent, Surface, island};
    use crate::ui::geometry::ShapeKind;
    use chrono::{NaiveTime, TimeZone};
    use std::time::Instant;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn design_scan() -> ScanResult {
        let gb = |tenths: u64| CatSize {
            bytes: tenths * GIB / 10,
            files: 1,
            measured: true,
        };
        let unknown = CatSize::default();
        ScanResult {
            cats: [gb(1), gb(18), gb(4), gb(1), unknown, unknown, unknown],
            skipped_browsers: vec![],
        }
    }

    fn mole_view<'a>(
        phase: MolePhase,
        summary: Option<&'a crate::mole::PreviewSummary>,
        log: &'a std::collections::VecDeque<String>,
    ) -> super::super::MoleView<'a> {
        super::super::MoleView {
            phase,
            opts: crate::mole::MoleOpts::default(),
            show_log: false,
            version: Some("1.30.0"),
            tested: true,
            summary,
            log,
            report: None,
        }
    }

    #[test]
    fn mole_mode_shows_what_its_phase_allows() {
        use crate::mole::{GroupSize, MoleOpts, PreviewSummary};
        use crate::ui::panel_layout::MoleBody;
        let log = std::collections::VecDeque::new();
        let summary = PreviewSummary {
            groups: [
                GroupSize::Bytes(18 * GIB / 10),
                GroupSize::Unreadable,
                GroupSize::Bytes(GIB / 10),
                GroupSize::Bytes(GIB / 10),
            ],
            total: Some((24 * GIB / 10, 29)),
        };
        let opts = MoleOpts::default();
        let view = |phase, armed| CleanerView {
            mode: CleanMode::Mole,
            checked: [false; 7],
            armed,
            scan: None,
            scanning: false,
            busy: false,
            mole: mole_view(phase, Some(&summary), &log),
        };
        let previewed = MolePhase::Previewed { opts, ok: true };
        // Before the preview ends: the rows wait, nothing to run.
        for phase in [MolePhase::Detecting, MolePhase::Scanning(opts)] {
            let v = view(phase, false);
            let look = v.mole_look();
            assert_eq!(look.body, MoleBody::Rows);
            assert!(!look.go && look.opts_live && look.seg_live);
            assert_eq!(v.group(MoleGroup::Browser), GroupShown::Scanning);
            assert_eq!(
                mole_footer(Lang::Id, &v),
                "Memindai dengan Mole… ± 30 detik"
            );
        }
        // Previewed: sizes, "lihat log" where unreadable, the button live.
        let v = view(previewed, false);
        let look = v.mole_look();
        assert!(look.go && look.protect);
        assert_eq!(look.unreadable, [false, true, false, false]);
        assert_eq!(
            v.group(MoleGroup::Browser),
            GroupShown::Bytes(18 * GIB / 10)
        );
        assert_eq!(v.group(MoleGroup::Apps), GroupShown::SeeLog);
        assert_eq!(mole_footer(Lang::Id, &v), "Pratinjau: ± 2,4 GB");
        assert_eq!(mole_go_label(Lang::Id, &v), "Jalankan Mole");
        let armed = view(previewed, true);
        assert_eq!(mole_footer(Lang::Id, &armed), "± 2,4 GB · 29 item");
        assert_eq!(mole_go_label(Lang::Id, &armed), "Yakin? Hapus permanen");
        assert_eq!(mole_go_label(Lang::En, &armed), "Sure? Delete for good");
        // A preview with other options than ticked, or one that failed,
        // never makes the button live.
        let other = MolePhase::Previewed {
            opts: MoleOpts {
                system: true,
                ..opts
            },
            ok: true,
        };
        assert!(!view(other, false).mole_look().go);
        let failed = view(MolePhase::Previewed { opts, ok: false }, false);
        assert!(!failed.mole_look().go);
        assert_eq!(failed.group(MoleGroup::Dev), GroupShown::SeeLog);
        let no_summary = CleanerView {
            mole: mole_view(previewed, None, &log),
            ..view(previewed, false)
        };
        assert_eq!(no_summary.group(MoleGroup::Dev), GroupShown::SeeLog);
        assert_eq!(mole_footer(Lang::En, &no_summary), "Preview: see log");
        // The own mode's clean running: Mole's button waits.
        let busy = CleanerView {
            busy: true,
            ..view(previewed, false)
        };
        assert!(!busy.mole_look().go && !busy.mole_look().seg_live);
        // Running: the log, nothing but the way back.
        let running = view(MolePhase::Running, false).mole_look();
        assert_eq!(running.body, MoleBody::Log { result: false });
        assert!(!running.go && !running.opts_live && !running.seg_live && !running.protect);
        assert_eq!(
            mole_go_label(Lang::Id, &view(MolePhase::Running, false)),
            "Mole membersihkan…"
        );
        // Finished: the result over the log, "Pratinjau lagi".
        let done = view(MolePhase::Finished { ok: true }, false);
        assert_eq!(done.mole_look().body, MoleBody::Log { result: true });
        assert!(done.mole_look().go);
        assert_eq!(mole_go_label(Lang::Id, &done), "Pratinjau lagi");
        // Not installed: the card only.
        let missing = view(MolePhase::Missing, false).mole_look();
        assert_eq!(missing.body, MoleBody::Card);
        assert!(!missing.go && !missing.protect && !missing.opts_live && missing.seg_live);
        // The preview's log in place of the rows, and back.
        let shown = CleanerView {
            mole: super::super::MoleView {
                show_log: true,
                ..mole_view(previewed, Some(&summary), &log)
            },
            ..view(previewed, false)
        };
        assert_eq!(shown.mole_look().body, MoleBody::Log { result: false });
        assert!(shown.mole_look().log_toggle);
        assert_eq!(shown.mole_look().unreadable, [false; 4]);
    }

    #[test]
    fn cleaner_totals_and_labels() {
        let scan = design_scan();
        let no_log = std::collections::VecDeque::new();
        let mut view = CleanerView {
            mode: CleanMode::Own,
            checked: CleanCat::ALL.map(CleanCat::checked_at_start),
            armed: false,
            scan: Some(&scan),
            scanning: false,
            busy: false,
            mole: mole_view(MolePhase::Unknown, None, &no_log),
        };
        // The top three: 0,1 + 1,8 + 0,4.
        let top3 = GIB / 10 + 18 * GIB / 10 + 4 * GIB / 10;
        assert_eq!(view.picked_bytes(), top3);
        assert_eq!(go_label(Lang::Id, &view), "Bersihkan 2,3 GB");
        assert!(view.go_live());
        assert_eq!(view.size(CleanCat::DevCache), RowSize::Bytes(GIB / 10));
        assert_eq!(view.size(CleanCat::WindowsUpdate), RowSize::Unknown);
        // An unmeasured row adds nothing.
        view.checked[CleanCat::WindowsUpdate.index()] = true;
        assert_eq!(view.picked_bytes(), top3);
        view.armed = true;
        assert_eq!(go_label(Lang::Id, &view), "Yakin? Hapus 2,3 GB");
        assert_eq!(go_label(Lang::En, &view), "Sure? Delete 2.3 GB");
        // Scanning: every size waits, the button takes no click.
        let scanning = CleanerView {
            scanning: true,
            ..view
        };
        assert_eq!(scanning.size(CleanCat::TempLogs), RowSize::Scanning);
        assert!(!scanning.go_live());
        // A clean running: the button says so and takes no click.
        let busy = CleanerView { busy: true, ..view };
        assert_eq!(go_label(Lang::Id, &busy), "Membersihkan…");
        assert!(!busy.go_live());
        // Nothing ticked: no click; no scan yet: dashes.
        let none = CleanerView {
            checked: [false; 7],
            ..view
        };
        assert!(!none.go_live());
        let fresh = CleanerView { scan: None, ..view };
        assert_eq!(fresh.size(CleanCat::TempLogs), RowSize::Unknown);
        assert_eq!(fresh.picked_bytes(), 0);
        assert_eq!(clean_icon(CleanCat::WindowsUpdate), Icon::Windows);
        // A Mole clean running: the own mode's button waits.
        let mole_running = CleanerView {
            mole: mole_view(MolePhase::Running, None, &no_log),
            ..view
        };
        assert!(!mole_running.go_live());
    }

    /// The island's own notices about cleaning, measured with the embedded
    /// DM Sans as drawn (`Renderer::text_width`: the title's 12.5 medium,
    /// the body's 10.5, tabular figures), fit the notice's text boxes in
    /// both languages, for realistic worst cases. The 360 × 76 notice keeps
    /// its size; what would not fit in a body is left out, least
    /// important first, never cut with "…".
    #[test]
    fn cleaning_notices_fit_the_notice() {
        use crate::clean::{AdminOutcome, CleanReport};
        use crate::mole::{CleanRun, MoleReport};
        use crate::ui::fonts;
        use crate::ui::geometry::{ShapeKind, notice_action_w, notice_layout};
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let r = Renderer::new().expect("renderer");
        assert!(
            fonts::embedded(&r.dwrite).loaded.dm_sans,
            "the test binary carries the embedded fonts"
        );
        let gb = |x: f64| (x * GIB as f64) as u64;
        // A notice of `kind` drawn: title and body inside their boxes.
        let check = |n: &crate::model::Notice| {
            for style in [Style::Pill, Style::Notch] {
                let s = shape_for(style, ShapeKind::Notice);
                assert_eq!((s.w, s.h), (360.0, 76.0));
                let chip =
                    crate::ui::geometry::notice_action(&n.kind, crate::pomodoro::PomoStatus::Idle)
                        .map(|_| notice_action_w(&n.kind));
                let l = notice_layout(s, chip);
                let tw = r.text_width(&n.title, &r.fonts.title);
                assert!(tw <= l.title.w, "{:?}: {tw} px in {}", n.title, l.title.w);
                let bw = r.text_width(&n.body, &r.fonts.small);
                assert!(bw <= l.body.w, "{:?}: {bw} px in {}", n.body, l.body.w);
            }
        };
        // The old body was cut off: the test can tell.
        let body_w = notice_layout(shape_for(Style::Pill, ShapeKind::Notice), None)
            .body
            .w;
        let old = "Disk C: sekarang sisa 49,5 GB · 3 item dilewati (sedang dipakai)";
        assert!(r.text_width(old, &r.fonts.small) > body_w);

        let browsers: [&[&'static str]; 3] = [
            &[],
            &["Chrome"],
            &["Chrome", "Edge", "Firefox", "Brave", "Opera"],
        ];
        for lang in [Lang::Id, Lang::En] {
            let kind = NoticeKind::CleanDone;
            let fits = |s: &str| r.notice_text_fits(&kind, false, s);
            for freed in [gb(2.3), gb(12.3), gb(98.7)] {
                for (drive, skipped) in [(49.5, 3), (12.3, 123), (99.9, 250)] {
                    for open in browsers {
                        for admin in [AdminOutcome::NotAsked, AdminOutcome::Cancelled] {
                            let report = CleanReport {
                                freed,
                                drive: Some(('C', gb(drive))),
                                skipped,
                                skipped_browsers: open.to_vec(),
                                admin,
                            };
                            let n = crate::clean::done_notice(lang, &report, &fits);
                            check(&n);
                            // The drive and what was skipped always stay.
                            let free = i18n::format_size(lang, gb(drive));
                            assert!(n.body.contains("C: "), "{:?}", n.body);
                            assert!(n.body.contains(&free), "{:?}", n.body);
                            assert!(n.body.contains(&skipped.to_string()), "{:?}", n.body);
                        }
                    }
                }
            }
            // The design's case keeps everything.
            let design = CleanReport {
                freed: gb(2.3),
                drive: Some(('C', gb(49.5))),
                skipped: 3,
                skipped_browsers: vec![],
                admin: AdminOutcome::NotAsked,
            };
            let n = crate::clean::done_notice(lang, &design, &fits);
            match lang {
                // The Indonesian wording that was cut off now takes its
                // shorter form, both parts kept.
                Lang::Id => assert_eq!(n.body, "C: sisa 49,5 GB · 3 item dilewati"),
                Lang::En => {
                    assert!(n.body.contains("49.5 GB free"), "{:?}", n.body);
                    assert!(n.body.contains("3 items skipped"), "{:?}", n.body);
                }
            }

            // Mole: done (two-digit GB, the system part skipped), failed
            // with a code (an NTSTATUS one too) or stopped.
            for freed in [gb(2.4), gb(12.4), gb(99.9), gb(123.4), gb(1500.0)] {
                for (exit, admin) in [
                    (Some(0), AdminOutcome::NotAsked),
                    (Some(0), AdminOutcome::Cancelled),
                    (Some(1), AdminOutcome::NotAsked),
                    (Some(-1_073_741_510), AdminOutcome::NotAsked),
                    (None, AdminOutcome::NotAsked),
                    (Some(0), AdminOutcome::Failed),
                ] {
                    let report = MoleReport {
                        run: CleanRun { exit, admin },
                        freed,
                        drive: Some(('C', gb(47.2), gb(49.6))),
                    };
                    let kind = if report.run.ok() {
                        NoticeKind::MoleDone
                    } else {
                        NoticeKind::MoleFailed
                    };
                    let fits = |title: bool, s: &str| r.notice_text_fits(&kind, title, s);
                    let n = crate::mole::done_notice(lang, &report, &fits);
                    assert_eq!(n.kind, kind);
                    check(&n);
                }
            }
            // The design's notice (d2) keeps its full wording in both
            // languages beside the "Log" chip.
            let design = MoleReport {
                run: CleanRun {
                    exit: Some(0),
                    admin: AdminOutcome::NotAsked,
                },
                freed: gb(2.4),
                drive: Some(('C', gb(47.2), gb(49.6))),
            };
            let kind = NoticeKind::MoleDone;
            let fits = |title: bool, s: &str| r.notice_text_fits(&kind, title, s);
            let n = crate::mole::done_notice(lang, &design, &fits);
            let (title, body) = match lang {
                Lang::Id => (
                    "Mole selesai · 2,4 GB dibebaskan",
                    "Diukur dari sisa disk C:",
                ),
                Lang::En => ("Mole done · 2.4 GB freed", "Measured from free space on C:"),
            };
            assert_eq!((n.title.as_str(), n.body.as_str()), (title, body));
        }
    }

    #[test]
    fn utf16_offsets_map_to_whole_characters() {
        assert_eq!(utf16_prefix("git status", 3), "git");
        assert_eq!(utf16_prefix("git", 0), "");
        assert_eq!(utf16_prefix("git", 9), "git");
        // "é" is one unit, "😀" two: an offset inside it stops before it.
        assert_eq!(utf16_prefix("é😀x", 1), "é");
        assert_eq!(utf16_prefix("é😀x", 2), "é");
        assert_eq!(utf16_prefix("é😀x", 3), "é😀");
    }

    fn cli_view<'a>(
        output: &'a crate::cli::Output,
        echoes: &'a crate::cli_panel::Echoes,
        run: RunState,
    ) -> CliView<'a> {
        CliView {
            output,
            echoes,
            scroll: 0,
            run,
            shell: crate::cli::Shell::PowerShell,
            cwd: "D:\\Proyek",
            input: "",
            selection: (0, 0),
            focused: true,
            suggestions: Vec::new(),
            selected: 0,
            tall: false,
            from_explorer: false,
            menu: None,
        }
    }

    #[test]
    fn the_explorer_chip_leads_while_idle_or_ended() {
        let (output, echoes) = Default::default();
        let t0 = Instant::now();
        let looks = |run, lang| {
            let view = CliView {
                from_explorer: true,
                ..cli_view(&output, &echoes, run)
            };
            cli_chips(lang, &view, t0)
                .into_iter()
                .map(|c| (c.text, c.look))
                .collect::<Vec<_>>()
        };
        let explorer = ("dari Explorer".to_owned(), ChipLook::Explorer);
        let folder = ("D:\\Proyek".to_owned(), ChipLook::Path);
        assert_eq!(
            looks(RunState::Idle, Lang::Id),
            [
                explorer.clone(),
                folder.clone(),
                ("PowerShell".to_owned(), ChipLook::Mono)
            ]
        );
        assert_eq!(
            looks(RunState::Idle, Lang::En)[0],
            ("from Explorer".to_owned(), ChipLook::Explorer)
        );
        // Running: only the run's chip, as before.
        let running = looks(RunState::Running { since: t0 }, Lang::Id);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].1, ChipLook::Run);
        let ended = looks(
            RunState::Ended {
                code: Some(0),
                killed: false,
                took: Duration::from_millis(2_100),
            },
            Lang::Id,
        );
        assert_eq!(ended.len(), 4);
        assert_eq!((&ended[0], &ended[1]), (&explorer, &folder));
    }

    #[test]
    fn cli_chips_follow_the_run() {
        let (output, echoes) = Default::default();
        let t0 = Instant::now();
        let looks = |run| {
            cli_chips(
                Lang::Id,
                &cli_view(&output, &echoes, run),
                t0 + Duration::from_secs(42),
            )
            .into_iter()
            .map(|c| (c.text, c.look))
            .collect::<Vec<_>>()
        };
        let folder = ("D:\\Proyek".to_owned(), ChipLook::Path);
        let shell = ("PowerShell".to_owned(), ChipLook::Mono);
        assert_eq!(looks(RunState::Idle), [folder.clone(), shell.clone()]);
        assert_eq!(
            looks(RunState::Running { since: t0 }),
            [("Berjalan · 0:42".to_owned(), ChipLook::Run)]
        );
        let ended = |code, killed| RunState::Ended {
            code,
            killed,
            took: Duration::from_millis(2_100),
        };
        assert_eq!(
            looks(ended(Some(0), false)),
            [
                folder.clone(),
                ("selesai · 2,1 dtk".to_owned(), ChipLook::Ok),
                shell.clone()
            ]
        );
        assert_eq!(
            looks(ended(Some(1), false))[1],
            ("exit 1 · 2,1 dtk".to_owned(), ChipLook::Err)
        );
        assert_eq!(looks(ended(Some(0), true))[1].1, ChipLook::Err, "stopped");
    }

    #[test]
    fn track_time_reads_like_a_player() {
        assert_eq!(track_time(0), "0:00");
        assert_eq!(track_time(999), "0:00", "whole seconds, rounded down");
        assert_eq!(track_time(102_000), "1:42");
        assert_eq!(track_time(238_500), "3:58");
        assert_eq!(track_time(600_000), "10:00");
        assert_eq!(track_time(3_599_999), "59:59");
        assert_eq!(track_time(3_600_000), "1:00:00");
        assert_eq!(track_time(3_723_000), "1:02:03");
        assert_eq!(track_time(36_000_000), "10:00:00");
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(d: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
        d.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap())
    }

    fn event(start: EventTime, end: EventTime) -> CalEvent {
        CalEvent {
            id: "e".into(),
            calendar_id: "primary".into(),
            title: "e".into(),
            start,
            end,
            description: String::new(),
            color: None,
            editable: true,
        }
    }

    fn local(t: NaiveDateTime) -> EventTime {
        EventTime::At(Local.from_local_datetime(&t).earliest().unwrap())
    }

    #[test]
    fn event_times() {
        let l = Lang::Id;
        let d = date(2026, 10, 25);
        let one = event(EventTime::AllDay(d), EventTime::AllDay(date(2026, 10, 26)));
        assert_eq!(event_time_label(l, &one), "Seharian");
        let multi = event(
            EventTime::AllDay(date(2026, 10, 24)),
            EventTime::AllDay(date(2026, 10, 27)),
        );
        assert_eq!(event_time_label(l, &multi), "Seharian · 24 Okt – 26 Okt");
        let timed = event(local(at(d, 10, 0)), local(at(d, 11, 30)));
        assert_eq!(event_time_label(l, &timed), "10.00 – 11.30");
        let night = event(local(at(d, 22, 0)), local(at(date(2026, 10, 26), 2, 0)));
        assert_eq!(event_time_label(l, &night), "25 Okt 22.00 – 26 Okt 02.00");
    }

    #[test]
    fn event_times_in_english() {
        let l = Lang::En;
        let d = date(2026, 10, 25);
        let one = event(EventTime::AllDay(d), EventTime::AllDay(date(2026, 10, 26)));
        assert_eq!(event_time_label(l, &one), "All day");
        let multi = event(
            EventTime::AllDay(date(2026, 10, 24)),
            EventTime::AllDay(date(2026, 10, 27)),
        );
        assert_eq!(event_time_label(l, &multi), "All day · 24 Oct – 26 Oct");
        let night = event(local(at(d, 22, 0)), local(at(date(2026, 10, 26), 2, 0)));
        assert_eq!(event_time_label(l, &night), "25 Oct 22:00 – 26 Oct 02:00");
    }

    #[test]
    fn event_colors_default_by_kind() {
        let p = island(Accent::Plum, Surface::Espresso);
        let d = date(2026, 10, 25);
        let all_day = event(EventTime::AllDay(d), EventTime::AllDay(date(2026, 10, 26)));
        assert_eq!(event_color(&all_day, &p), p.sage);
        let timed = event(local(at(d, 10, 0)), local(at(d, 11, 0)));
        assert_eq!(event_color(&timed, &p), p.accent);
        let own = CalEvent {
            color: Some(0x123456),
            ..timed
        };
        assert_eq!(event_color(&own, &p), 0x123456);
    }

    #[test]
    fn note_times_relative_to_today() {
        let l = Lang::Id;
        let today = date(2026, 10, 25);
        assert_eq!(
            note_time_label(l, at(today, 9, 12), today),
            "hari ini 09.12"
        );
        assert_eq!(
            note_time_label(l, at(date(2026, 10, 24), 23, 0), today),
            "kemarin"
        );
        assert_eq!(
            note_time_label(l, at(date(2026, 10, 23), 8, 0), today),
            "23 Okt"
        );
        assert_eq!(
            note_time_label(l, at(date(2025, 10, 23), 8, 0), today),
            "23 Okt 2025"
        );
    }

    #[test]
    fn note_times_in_english() {
        let l = Lang::En;
        let today = date(2026, 10, 25);
        assert_eq!(note_time_label(l, at(today, 9, 12), today), "Today 09:12");
        assert_eq!(
            note_time_label(l, at(date(2026, 10, 24), 23, 0), today),
            "Yesterday"
        );
        assert_eq!(
            note_time_label(l, at(date(2025, 10, 23), 8, 0), today),
            "23 Oct 2025"
        );
    }

    #[test]
    fn note_cards_split_title_and_snippet() {
        let text = "\n  Ide fasad  \nKisi-kisi kayu vertikal,\n\n  jarak 8 cm.\n";
        assert_eq!(first_line(text), "Ide fasad");
        assert_eq!(note_snippet(text), "Kisi-kisi kayu vertikal, jarak 8 cm.");
        assert_eq!(note_snippet("Hanya judul"), "");
        assert_eq!(note_snippet(""), "");
    }

    #[test]
    fn reminder_days_relative_to_today() {
        let today = date(2026, 10, 26);
        assert_eq!(day_label(Lang::Id, today, today), "Hari ini");
        assert_eq!(day_label(Lang::Id, date(2026, 10, 27), today), "Besok");
        assert_eq!(
            day_label(Lang::Id, date(2026, 10, 30), today),
            "Jum, 30 Okt"
        );
        assert_eq!(
            day_label(Lang::Id, date(2026, 10, 25), today),
            "Min, 25 Okt"
        );
        assert_eq!(day_label(Lang::Id, date(2027, 1, 3), today), "3 Jan 2027");
        assert_eq!(day_label(Lang::En, today, today), "Today");
        assert_eq!(day_label(Lang::En, date(2026, 10, 27), today), "Tomorrow");
        assert_eq!(
            day_label(Lang::En, date(2026, 10, 30), today),
            "Fri, 30 Oct"
        );
    }

    #[test]
    fn reminder_rows_show_time_and_first_line() {
        let today = date(2026, 10, 26);
        let unix = |t: NaiveDateTime| {
            Local
                .from_local_datetime(&t)
                .earliest()
                .unwrap()
                .timestamp()
        };
        let r = Reminder {
            id: "r".into(),
            text: "\nMinum obat\nsetelah makan".into(),
            due_unix: unix(at(today, 8, 5)),
            repeat: Repeat::Daily,
            done: false,
            days: Vec::new(),
        };
        assert_eq!(reminder_row(Lang::Id, &r), ("08.05".into(), "Minum obat"));
        assert_eq!(reminder_row(Lang::En, &r), ("08:05".into(), "Minum obat"));
    }

    #[test]
    fn hour_labels_follow_the_language() {
        assert_eq!(hour_label(Lang::Id, 9), "09.00");
        assert_eq!(hour_label(Lang::En, 13), "13:00");
        assert_eq!(hour_label(Lang::Id, 24), "24.00");
    }

    #[test]
    fn calendar_titles_per_view() {
        let d = date(2026, 10, 26);
        assert_eq!(
            calendar_title(Lang::Id, CalView::Month, d, d),
            "Oktober 2026"
        );
        assert_eq!(
            calendar_title(Lang::Id, CalView::Week, d, d),
            "26 Okt – 1 Nov 2026"
        );
        assert_eq!(
            calendar_title(Lang::Id, CalView::Day, d, d),
            "Senin, 26 Oktober"
        );
    }

    #[test]
    fn more_lines() {
        assert_eq!(more_label(Lang::Id, 0), "Kembali ke atas");
        assert_eq!(more_label(Lang::Id, 4), "+4 lagi");
        assert_eq!(more_label(Lang::En, 0), "Back to top");
        assert_eq!(more_label(Lang::En, 4), "+4 more");
    }

    #[test]
    fn first_line_skips_blank_lines() {
        assert_eq!(first_line("\n  \n  Beli tinta  \nlain"), "Beli tinta");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn mmss_rounds_up_and_allows_long_phases() {
        assert_eq!(mmss(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(mmss(Duration::from_millis(1_121_400)), "18:42");
        assert_eq!(mmss(Duration::from_secs(120 * 60)), "120:00");
        assert_eq!(mmss(Duration::ZERO), "00:00");
    }

    #[test]
    fn pomodoro_labels_by_status() {
        let l = Lang::Id;
        let pal = island(Accent::Terracotta, Surface::Espresso);
        let t0 = Instant::now();
        let mut p = Pomodoro::new(PomoSettings::default());
        assert_eq!(primary_icon(p.status()), Icon::Play);
        assert_eq!(pal.phase(p.phase()), pal.accent);
        assert_eq!(next_phase(&p), (Phase::ShortBreak, 5));
        p.start_pause(t0);
        assert_eq!(compact_label(l, &p), "Fokus 1/4");
        assert_eq!(primary_icon(p.status()), Icon::Pause);
        assert_eq!(phase_label(l, &p), "Fokus");
        p.start_pause(t0);
        assert_eq!(primary_icon(p.status()), Icon::Play);
        assert_eq!(phase_label(l, &p), "Fokus · dijeda");
        p.skip(t0);
        assert_eq!(compact_label(l, &p), "Siap · klik");
        assert_eq!(phase_label(l, &p), "Istirahat siap");
        assert_eq!(primary_icon(p.status()), Icon::Play);
        assert_eq!(pal.phase(p.phase()), pal.sage);
        assert_eq!(next_phase(&p), (Phase::Focus, 25));
        p.start_pause(t0);
        assert_eq!(compact_label(l, &p), "Istirahat");
    }

    #[test]
    fn the_last_session_leads_to_the_long_break() {
        let t0 = Instant::now();
        let mut p = Pomodoro::new(PomoSettings::default());
        // Focus 1, break, focus 2, break, focus 3, break, focus 4.
        for _ in 0..6 {
            p.skip(t0);
        }
        assert_eq!((p.phase(), p.session()), (Phase::Focus, 4));
        assert_eq!(next_phase(&p), (Phase::LongBreak, 15));
    }

    #[test]
    fn pomodoro_labels_in_english() {
        let l = Lang::En;
        let t0 = Instant::now();
        let mut p = Pomodoro::new(PomoSettings::default());
        p.start_pause(t0);
        assert_eq!(compact_label(l, &p), "Focus 1/4");
        p.start_pause(t0);
        assert_eq!(phase_label(l, &p), "Focus · paused");
        p.skip(t0);
        assert_eq!(compact_label(l, &p), "Ready · click");
        assert_eq!(phase_label(l, &p), "Break ready");
    }

    /// `s` drawn in `format` in `area` by `r` fits on one line and its ink
    /// (left, top, right, bottom) stays inside `area`, so the clip cuts
    /// nothing.
    fn ink_fits(
        r: &Renderer,
        s: &str,
        format: &IDWriteTextFormat,
        area: Rect,
    ) -> (f32, f32, f32, f32) {
        let w = r.text_width(s, format);
        assert!(w <= area.w, "{s:?}: {w} px in a {} px box", area.w);
        let wide: Vec<u16> = s.encode_utf16().collect();
        let layout = r.layout(&wide, format, area.w, area.h).expect("layout");
        let o = unsafe { layout.GetOverhangMetrics() }.expect("overhang");
        let ink = (
            area.x - o.left,
            area.y - o.top,
            area.right() + o.right,
            area.bottom() + o.bottom,
        );
        assert!(
            o.left <= 0.0 && o.top <= 0.0 && o.right <= 0.0 && o.bottom <= 0.0,
            "{s:?} ink {ink:?} outside {area:?}"
        );
        ink
    }

    /// Display text measured with the real embedded DM Sans 600 (tracked) and
    /// JetBrains Mono 600 (not tracked), through the renderer's own layout
    /// path, fits the boxes the layouts keep for it: no ellipsis, no ink
    /// clipped. When a font file, size or the tracking changes and this
    /// fails, re-measure the constants (`CLOCK_W`, the pomodoro ring's
    /// `TIME_*`, `CLI_TITLE_W`, the panel layout tests' `TITLE_W`, and for
    /// Mono `MONO_CLOCK` and `MONO_BIG_TIME`).
    #[test]
    fn display_text_fits_its_boxes() {
        use crate::ui::fonts;
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

        // WIC, made by the renderer, needs COM on this thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut r = Renderer::new().expect("renderer");
        if !fonts::embedded(&r.dwrite).loaded.dm_sans {
            eprintln!("skipped: the embedded fonts are not in this binary");
            return;
        }
        display_fits(&r);
        assert!(
            fonts::embedded(&r.dwrite).loaded.jetbrains_mono,
            "the test binary carries JetBrains Mono"
        );
        r.set_ui_font(UiFont::Mono).expect("mono formats");
        display_fits(&r);
    }

    /// The notice chips (their widths are constants: `ACTION_W`,
    /// `LOG_ACTION_W`, `RELEASES_ACTION_W`) hold their words in both
    /// languages in DM Sans and in JetBrains Mono, text and ink inside.
    #[test]
    fn notice_action_chips_hold_their_words_in_both_fonts() {
        use crate::ui::fonts;
        use crate::ui::geometry::{notice_action, notice_action_w};
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut r = Renderer::new().expect("renderer");
        if !fonts::embedded(&r.dwrite).loaded.jetbrains_mono {
            eprintln!("skipped: the embedded fonts are not in this binary");
            return;
        }
        let kinds = [
            (
                NoticeKind::Reminder {
                    id: "1".into(),
                },
                crate::pomodoro::PomoStatus::Idle,
            ),
            (NoticeKind::Pomodoro, crate::pomodoro::PomoStatus::Ready),
            (NoticeKind::SystemLow, crate::pomodoro::PomoStatus::Idle),
            (
                NoticeKind::DevicePlugged { letter: 'E' },
                crate::pomodoro::PomoStatus::Idle,
            ),
            (
                NoticeKind::ClipPreview { id: 1 },
                crate::pomodoro::PomoStatus::Idle,
            ),
            (NoticeKind::MoleDone, crate::pomodoro::PomoStatus::Idle),
            (
                NoticeKind::UpdateAvailable {
                    version: "3.12.0".into(),
                },
                crate::pomodoro::PomoStatus::Idle,
            ),
            (NoticeKind::UpdateFailed, crate::pomodoro::PomoStatus::Idle),
        ];
        for ui in [UiFont::Sans, UiFont::Mono] {
            r.set_ui_font(ui).expect("font");
            for (kind, pomo) in &kinds {
                let key = notice_action(kind, *pomo).expect("a chip");
                let chip = Rect {
                    x: 0.0,
                    y: 0.0,
                    w: notice_action_w(kind),
                    h: 24.0,
                };
                for lang in [Lang::Id, Lang::En] {
                    let label = text(lang, key);
                    ink_fits(&r, label, &r.fonts.button_center, chip);
                }
            }
        }
    }

    /// `set_ui_font` switches every format and the tracking at once, does
    /// nothing when the font is the one in effect, and Sans comes back
    /// exactly as it was (the lyric line is measured again with the new font,
    /// which is what the notch's width follows).
    #[test]
    fn set_ui_font_switches_formats_and_tracking_and_back() {
        use crate::ui::fonts;
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut r = Renderer::new().expect("renderer");
        if !fonts::embedded(&r.dwrite).loaded.jetbrains_mono {
            eprintln!("skipped: the embedded fonts are not in this binary");
            return;
        }
        assert_eq!(r.ui_font(), UiFont::Sans);
        let line = "Senja di Kemang, sampai jadi debu";
        let sans = (
            r.text_width("Oktober 2026", &r.fonts.heading),
            r.lyric_text_width(line).expect("lyric width"),
            r.text_width("Minum vitamin", &r.fonts.secondary),
        );
        // Sans is unchanged: the tracked heading is the pinned 127 px.
        assert!((sans.0 - 127.0).abs() < 1.0, "{}", sans.0);
        r.set_ui_font(UiFont::Sans).expect("same font");
        assert_eq!(r.text_width("Oktober 2026", &r.fonts.heading), sans.0);
        r.set_ui_font(UiFont::Mono).expect("mono");
        assert_eq!(r.ui_font(), UiFont::Mono);
        // JetBrains Mono: 0.6 em a character and no tracking (12 cells of 12 px).
        let heading = r.text_width("Oktober 2026", &r.fonts.heading);
        assert!((heading - 144.0).abs() < 0.05, "{heading}");
        let lyric = r.lyric_text_width(line).expect("lyric width");
        let cells = line.chars().count() as f32;
        assert!((lyric - cells * 0.6 * MONO_LYRIC).abs() < 0.1, "{lyric}");
        assert!(lyric > sans.1);
        let body = r.text_width("Minum vitamin", &r.fonts.secondary);
        assert!((body - 13.0 * 0.6 * 12.0).abs() < 0.1, "{body}");
        // And back: exactly the Sans measures of before.
        r.set_ui_font(UiFont::Sans).expect("sans");
        assert_eq!(r.ui_font(), UiFont::Sans);
        assert_eq!(r.text_width("Oktober 2026", &r.fonts.heading), sans.0);
        assert_eq!(r.lyric_text_width(line).expect("lyric width"), sans.1);
        assert_eq!(r.text_width("Minum vitamin", &r.fonts.secondary), sans.2);
    }

    /// The lyric line's size in Mono (`DisplaySizes::MONO`).
    const MONO_LYRIC: f32 = 16.0;

    /// The Mono display sizes are pinned: `MONO_CLOCK` and `MONO_BIG_TIME` are
    /// the largest 0.5 px steps (JetBrains Mono 600) whose widest text fits
    /// its box with the ink inside, and the widths they measure are these.
    /// Measured 2026-11-02 with JetBrains Mono v2.304: "00:00" at 14.5 px is
    /// 43.5 px in `CLOCK_W` (44); "100:00" at 46.5 px is 167.4 px in the
    /// ring's 167.5 px time box (15 px and 47 px do not fit).
    #[test]
    fn mono_display_sizes_are_the_largest_that_fit() {
        use crate::ui::fonts;
        use crate::ui::geometry::{CLOCK_W, idle_layout, shape_with_robot};
        use windows::Win32::Graphics::DirectWrite::{
            DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER,
        };
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut r = Renderer::new().expect("renderer");
        if !fonts::embedded(&r.dwrite).loaded.jetbrains_mono {
            eprintln!("skipped: the embedded fonts are not in this binary");
            return;
        }
        r.set_ui_font(UiFont::Mono).expect("mono formats");
        let face = fonts::Face::for_role_in(&r.dwrite, fonts::Role::Display, UiFont::Mono);
        // Built as `Fonts::new` builds the centred display formats.
        let at = |size: f32| {
            let format =
                crate::ui::render::line_format(&r.dwrite, &face, DWRITE_FONT_WEIGHT_SEMI_BOLD, size)
                    .expect("format");
            unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER) }.expect("align");
            format
        };
        // Whether `fits` (it panics) holds, as a bool.
        let holds = |size: f32, run: &dyn Fn(&Renderer, &IDWriteTextFormat)| {
            let format = at(size);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&r, &format))).is_ok()
        };
        let clock_box = idle_layout(
            shape_with_robot(Style::Pill, ShapeKind::Idle, true),
            true,
            true,
        )
        .clock
        .expect("clock box");
        assert_eq!(clock_box.w, CLOCK_W);
        let clock = |r: &Renderer, format: &IDWriteTextFormat| {
            for time in ["00:00", "23:59", "12:00"] {
                ink_fits(r, time, format, clock_box);
            }
        };
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let l = pomodoro_layout(s, false);
        let (cx, cy) = l.ring_center;
        let inner = l.ring_radius - RING_STROKE / 2.0;
        let big = |r: &Renderer, format: &IDWriteTextFormat| {
            for time in ["100:00", "25:00", "60:00", "0:00"] {
                let (left, top, right, bottom) = ink_fits(r, time, format, l.time);
                for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
                    assert!((x - cx).hypot(y - cy) <= inner, "{time:?} past the ring: ({x},{y}) c ({cx},{cy}) inner {inner} box {:?}", l.time);
                }
                assert!(bottom <= l.phase.y, "{time:?} ink over the phase");
            }
        };
        let largest = |run: &dyn Fn(&Renderer, &IDWriteTextFormat)| {
            let mut size = 8.0;
            while holds(size + 0.5, run) {
                size += 0.5;
            }
            size
        };
        // Silence the expected panics of the probing.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let (best_clock, best_big) = (largest(&clock), largest(&big));
        std::panic::set_hook(hook);
        assert_eq!(MONO_CLOCK, best_clock, "largest clock size that fits");
        assert_eq!(MONO_BIG_TIME, best_big, "largest countdown size that fits");
        for (text, size, want) in [
            ("00:00", MONO_CLOCK, MONO_CLOCK_WIDTH),
            ("100:00", MONO_BIG_TIME, MONO_BIG_TIME_WIDTH),
        ] {
            let got = r.text_width(text, &at(size));
            assert!(
                (got - want).abs() < 0.05,
                "{text:?} at {size}: {got} px, pinned {want}"
            );
        }
    }

    /// "00:00" in JetBrains Mono 600 at `MONO_CLOCK`, and "100:00" at
    /// `MONO_BIG_TIME`, as measured (5 and 6 cells of 0.6 em).
    const MONO_CLOCK_WIDTH: f32 = 43.5;
    const MONO_BIG_TIME_WIDTH: f32 = 167.4;

    /// The checks of `display_text_fits_its_boxes` for the font `r` is set to.
    fn display_fits(r: &Renderer) {
        use crate::ui::geometry::{CLOCK_W, idle_layout, shape_with_robot};

        let f = &r.fonts;
        let fits = |s: &str, format: &IDWriteTextFormat, area: Rect| ink_fits(r, s, format, area);
        for style in [Style::Pill, Style::Notch] {
            // The idle clock, beside Bata (`CLOCK_W`) and alone.
            for robot in [false, true] {
                let s = shape_with_robot(style, ShapeKind::Idle, robot);
                let clock = idle_layout(s, robot, true).clock.expect("clock box");
                if robot {
                    assert_eq!(clock.w, CLOCK_W);
                }
                for time in ["00:00", "23:59", "12:00"] {
                    fits(time, &f.clock, clock);
                }
            }

            let s = shape_for(style, ShapeKind::Panel);
            for strip in [false, true] {
                // The pomodoro countdown: inside the ring's stroke, over the
                // phase line.
                let l = pomodoro_layout(s, strip);
                let (cx, cy) = l.ring_center;
                let inner = l.ring_radius - RING_STROKE / 2.0;
                for time in ["100:00", "25:00", "60:00", "0:00"] {
                    let (left, top, right, bottom) = fits(time, &f.big_time, l.time);
                    for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
                        let d = (x - cx).hypot(y - cy);
                        assert!(
                            d <= inner,
                            "{time:?} ink corner {d} past the ring's {inner}"
                        );
                    }
                    assert!(bottom <= l.phase.y, "{time:?} ink over the phase");
                }

                // Headings draw in the header row grown by `HEADING_BLEED`.
                let bleed = |header: Rect| Rect {
                    y: header.y - HEADING_BLEED,
                    h: header.h + 2.0 * HEADING_BLEED,
                    ..header
                };
                let head = CliHead::default();
                let cli = cli_layout(s, strip, &head);
                for lang in [Lang::Id, Lang::En] {
                    fits(text(lang, Key::Cli), &f.heading, bleed(cli.header));
                    // The calendar title's box is its measured width.
                    let anchor = NaiveDate::from_ymd_opt(2026, 9, 30).expect("date");
                    for view in [CalView::Day, CalView::Week, CalView::Month] {
                        let title = calendar_title(lang, view, anchor, anchor);
                        let header = Rect {
                            w: r.calendar_title_w(&title),
                            ..l.header
                        };
                        fits(&title, &f.heading, bleed(header));
                    }
                }
            }
        }
        // The panel layout tests' `TITLE_W` is the Sans title's width; the
        // Mono title is 12 cells of 12 px (plus the 1 px of slack).
        let want = match r.ui_font {
            UiFont::Sans => 128.0,
            UiFont::Mono => 145.0,
        };
        assert_eq!(r.calendar_title_w("Oktober 2026"), want);
    }
}
