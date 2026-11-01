use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::{Days, Months, NaiveDate};

use crate::calendar::range_for;
use crate::clean::{CleanCat, CleanMode};
use crate::config::OpenMode;
use crate::i18n::{self, Key, text};
use crate::lyrics;
use crate::model::{
    ArrangeSave, CalView, ClipAction, Effect, Event, Hit, MediaCommand, MediaInfo, Notice,
    NoticeKind, NotifAction, Page, Shortcut, ShotAction, Timeline, View,
};
use crate::mole::MoleOpts;
use crate::pages::PanelPages;
use crate::pomodoro::{Phase, PhaseDone, PomoSettings, PomoStatus, Pomodoro};
use crate::tiles::{ShortcutTiles, Tile};
use crate::ui::geometry::ANIM_MS;

/// How long the panel stays open after the user did something in it (an
/// editor closing, a click, a page turned) once nothing else holds it.
pub const ACTIVITY_LINGER: Duration = Duration::from_secs(4);

/// How long each temporary notice stays on screen, counted from when it becomes current.
pub const NOTICE_DURATION: Duration = Duration::from_secs(5);
/// The morning greeting goes a little sooner.
pub const GREETING_DURATION: Duration = Duration::from_secs(4);
/// "Disalin" is only a confirmation.
pub const COPIED_DURATION: Duration = Duration::from_millis(1500);
/// The copy preview (text lines, a thumbnail) needs a little more to be read.
pub const CLIP_NOTICE_DURATION: Duration = Duration::from_secs(3);

/// The Arranged notice: long enough to reach "Batalkan".
pub const ARRANGE_NOTICE_DURATION: Duration = Duration::from_secs(3);

/// "Vale X.Y.Z tersedia": long enough to reach "Update".
pub const UPDATE_NOTICE_DURATION: Duration = Duration::from_secs(8);
/// The download's percent stays this long after its last update, so a
/// stalled download fades instead of showing a frozen number.
pub const UPDATE_PROGRESS_DURATION: Duration = Duration::from_secs(15);

/// "Disimpan" and "Dihapus" after a screenshot's button.
pub const SHOT_DONE_DURATION: Duration = Duration::from_millis(2500);
/// The screenshot preview never expires under the pointer; once it leaves,
/// at least this much more, so a click is never beaten by the timeout.
pub const SHOT_LEAVE_GRACE: Duration = Duration::from_secs(2);

/// How long `notice` stays once it is current.
fn duration_of(notice: &Notice) -> Duration {
    match notice.kind {
        NoticeKind::Greeting { .. } => GREETING_DURATION,
        NoticeKind::Copied => COPIED_DURATION,
        NoticeKind::ClipPreview { .. } => CLIP_NOTICE_DURATION,
        NoticeKind::Screenshot { secs, .. } => Duration::from_secs(u64::from(secs)),
        NoticeKind::ShotSaved | NoticeKind::ShotDeleted => SHOT_DONE_DURATION,
        NoticeKind::Arranged { .. } => ARRANGE_NOTICE_DURATION,
        NoticeKind::UpdateAvailable { .. } | NoticeKind::UpdateFailed => UPDATE_NOTICE_DURATION,
        NoticeKind::UpdateProgress { .. } => UPDATE_PROGRESS_DURATION,
        _ => NOTICE_DURATION,
    }
}

/// The screenshot id of a preview notice.
fn shot_of(notice: &Notice) -> Option<u64> {
    match notice.kind {
        NoticeKind::Screenshot { shot, .. } => Some(shot),
        _ => None,
    }
}

/// The copy preview (`ClipPreview`) in `notice`.
fn is_clip_preview(notice: &Notice) -> bool {
    matches!(notice.kind, NoticeKind::ClipPreview { .. })
}

/// A notice the pointer can act on: a button or a click target. Hover must
/// not open the panel over it (the notice would shrink into a strip under the
/// pointer), and its expiry waits while the pointer is on it. Claude waiting
/// and a failed Mole run stay until clicked but only inform: hover still opens
/// the panel over them, or it could not be opened at all.
fn notice_has_action(notice: &Notice) -> bool {
    matches!(
        notice.kind,
        NoticeKind::Reminder { .. }
            | NoticeKind::MoleDone
            | NoticeKind::SystemLow
            | NoticeKind::Pomodoro
            | NoticeKind::Screenshot { .. }
            | NoticeKind::DevicePlugged { .. }
            | NoticeKind::ClipPreview { .. }
            | NoticeKind::Arranged { .. }
            | NoticeKind::UpdateAvailable { .. }
            | NoticeKind::UpdateFailed
            | NoticeKind::App {
                app_id: Some(_),
                ..
            }
    )
}

/// The download's percent notice.
fn is_update_progress(notice: &Notice) -> bool {
    matches!(notice.kind, NoticeKind::UpdateProgress { .. })
}

/// After hover opens the panel, its media buttons ignore clicks until the
/// opening animation (`ANIM_MS`) has finished and this much longer: a pointer
/// on its way to a browser tab must not land on play/pause as the panel
/// grows under it.
pub const MEDIA_HOVER_GUARD: Duration = Duration::from_millis(ANIM_MS + 300);

/// How long the cleaner's armed button ("Yakin? Hapus …") waits for the
/// second click before it turns back.
pub const CLEAN_CONFIRM: Duration = Duration::from_secs(4);

/// What the open panel shows that closing it resets: the cleaner, the CLI's
/// size and folder menu, the notes, reminders and day pages, the strip.
/// `App` keeps it from before the event that closed the panel, so the
/// closing panel fades out as it was, not as the reset left it
/// (`docs/memory/panel-linger-and-motion.md`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShownPanel {
    pub page: Page,
    pub cleaner: Option<CleanerUi>,
    pub mole: MolePhase,
    pub cli_tall: bool,
    pub cli_menu: bool,
    pub notes_page: usize,
    pub reminders_page: usize,
    pub day_page: usize,
    pub strip: Option<Notice>,
}

/// The cleaner over the System page: its mode, which rows are ticked and
/// whether its button is armed. Opened by "Bersihkan"; gone when the panel
/// closes, the page changes or its back button is clicked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanerUi {
    pub mode: CleanMode,
    /// By `CleanCat::index`.
    pub checked: [bool; 7],
    /// The first click armed the button (either mode's) until then; a
    /// second click before it cleans. Anything else disarms it.
    pub armed_until: Option<Instant>,
    /// Mole mode: the preview's log shows in place of the groups.
    pub mole_log: bool,
}

impl CleanerUi {
    fn new(mode: CleanMode) -> Self {
        Self {
            mode,
            checked: CleanCat::ALL.map(CleanCat::checked_at_start),
            armed_until: None,
            mole_log: false,
        }
    }

    pub fn armed(&self) -> bool {
        self.armed_until.is_some()
    }

    /// The ticked categories, in row order.
    pub fn picked(&self) -> Vec<CleanCat> {
        CleanCat::ALL
            .into_iter()
            .filter(|c| self.checked[c.index()])
            .collect()
    }
}

/// Where the Mole mode stands (`crate::mole`). Kept while the cleaner is
/// closed only for a clean (it goes on) and its end (the notice opens its
/// log); a preview stops with the cleaner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MolePhase {
    /// Not looked for since the cleaner last closed.
    Unknown,
    Detecting,
    /// Not installed: the "Cara memasang" card.
    Missing,
    /// A dry-run with these options runs.
    Scanning(MoleOpts),
    /// The dry-run with these options ended; `ok`: it finished well, so
    /// "Jalankan Mole" may arm.
    Previewed {
        opts: MoleOpts,
        ok: bool,
    },
    /// The confirmed clean runs (the log shows).
    Running,
    /// It ended; the log and the result show.
    Finished {
        ok: bool,
    },
}

/// One mouse-wheel notch, as Windows reports it.
const WHEEL_STEP: i32 = 120;

/// Key shared by all pomodoro notices, so a newer one replaces an older one.
const POMODORO_KEY: &str = "pomodoro";

/// Decides what the island shows. Pure logic: OS sources feed it `Event`s and the
/// renderer draws its `View`; the caller performs the returned `Effect`s.
pub struct IslandState {
    media: Option<MediaInfo>,
    /// The media session's last reported timeline, as the watcher sent it
    /// (re-based here when play/pause flips).
    timeline: Option<Timeline>,
    /// "Lirik di notch" is on.
    lyrics_on: bool,
    /// `App` has synced lyrics for the current track.
    lyrics_ready: bool,
    queue: VecDeque<Notice>,
    current: Option<(Notice, Instant)>,
    /// Sticky notices, oldest first; the newest is shown. At most one
    /// `ClaudeWaiting` per session and one `Reminder` per reminder.
    sticky: Vec<Notice>,
    fullscreen: bool,
    hover: bool,
    /// The visit began over a notice with a button: hover does not open the
    /// panel (a wheel notch does) until the pointer leaves.
    hover_blocked: bool,
    /// The pointer's visit began on the current notice: only then does its
    /// expiry wait for the pointer. A notice that arrives under a pointer
    /// already there (over an open panel, say) runs its time as usual.
    visit_on_notice: bool,
    /// Susun's pick step is running: it keeps the panel open.
    arrange_pick: bool,
    /// The Arranged notice's name field is up: the notice waits.
    arrange_naming: bool,
    /// The sticky reminder that already kept one visit's hover off: the
    /// next visit opens the panel, with the reminder in its strip.
    reminder_seen: Option<Notice>,
    /// "Mode buka": how the pointer opens the panel.
    open_mode: OpenMode,
    /// The panel was opened to stay (hover-and-stay, or a click): it closes
    /// on a click outside the island, not when the pointer leaves.
    latched: bool,
    /// When hover last opened the closed panel; cleared when it closes.
    /// The media buttons wait `MEDIA_HOVER_GUARD` from then.
    hover_opened: Option<Instant>,
    /// An editor window is open; it keeps the panel open like hover.
    editor_open: bool,
    /// The CLI page's input has the keyboard; it keeps the panel open like
    /// an editor.
    cli_focused: bool,
    /// The CLI page shows the enlarged panel; back to small when the panel
    /// closes.
    cli_tall: bool,
    /// How many lines the CLI output is scrolled up from the newest; 0
    /// follows new output. Reset when the panel closes.
    cli_scroll: usize,
    /// The CLI page's folder menu is open. It never takes the keyboard;
    /// closed by a pick, a click beside it, Esc, a page change or the
    /// panel closing.
    cli_menu: bool,
    /// The Clipboard search field has the keyboard; it keeps the panel open
    /// like the CLI input.
    clip_focused: bool,
    /// The Notifikasi page's hidden input has the keyboard; it keeps the
    /// panel open like the CLI input.
    notif_focused: bool,
    /// "Hapus semua" was clicked once: armed ("Yakin?") until then, a second
    /// click before it clears the history. Anything else disarms it.
    notif_armed: Option<Instant>,
    /// A hotkey opened the panel on this page (the Clipboard's or the
    /// Devices'): it stays open (no hover, no keyboard) until the pointer
    /// comes (then hover holds it), the hotkey again, a copy, or
    /// `ClipHoldEnd`.
    hotkey_held: Option<Page>,
    /// Files are being dragged over the island: the panel is open on the
    /// shelf for them. Hover takes over like it does from `clip_held`;
    /// `ShelfDrag(false)` ends it.
    shelf_held: bool,
    /// Hover mode only: files were dropped and the panel was latched for
    /// them (`ShelfDropped`). The visit they were dropped in does not end it;
    /// the next one hands the panel to hover. Always `No` unless `latched`.
    shelf_drop: DropVisit,
    /// The quick search is open (over `search_prev`'s page); it holds the
    /// panel open like the CLI input does.
    search_open: bool,
    search_prev: Page,
    /// A shortcut tile was clicked: the panel stays shut, whatever keeps it
    /// open (an editor; the CLI input lets go), until the pointer leaves the island
    /// and comes back. Leaving alone is not enough: once the panel shrinks
    /// the pointer is over transparent pixels, so the leave arrives at the
    /// next small move, before the screen clip or picker is up.
    collapsed: Collapse,
    pomodoro: Pomodoro,
    /// The panel page, kept across closes so the panel reopens where it was left.
    page: Page,
    /// "Halaman panel": the order the wheel walks and the pages it skips.
    pages: PanelPages,
    /// "Pintasan": the tiles the Pintasan page shows.
    tiles: ShortcutTiles,
    /// Wheel delta not yet worth a whole page; cleared when the panel closes.
    wheel: i32,
    notes_page: usize,
    reminders_page: usize,
    /// Day view: which chunk of the day's events "+N lagi" has paged to.
    day_page: usize,
    today: NaiveDate,
    cal_view: CalView,
    cal_anchor: NaiveDate,
    cal_selected: NaiveDate,
    /// The cleaner, while it shows over the System page.
    cleaner: Option<CleanerUi>,
    /// A clean asked for has not finished: the cleaner's rows and button
    /// take no clicks meanwhile.
    clean_busy: bool,
    /// The cleaner's mode last used ("Pilih sendiri" by default).
    clean_mode: CleanMode,
    mole: MolePhase,
    /// "+ Sistem (admin)" and "+ Media game > 90 hari".
    mole_opts: MoleOpts,
    /// A Mole notice was clicked: the next panel on the System page opens
    /// the cleaner on Mole's log.
    mole_pending: bool,
    /// The pointer visited the current screenshot preview, or its file was
    /// opened: its expiry is no longer "ignored", so it never deletes.
    shot_touched: bool,
    /// The panel stays open until then after something the user did in it
    /// (`ACTIVITY_LINGER`). Never opens a closed panel.
    linger_until: Option<Instant>,
    /// `linger_until` is still in the future, as of the last `apply`
    /// (`panel_open` has no clock of its own).
    lingering: bool,
}

impl IslandState {
    pub fn new(today: NaiveDate, pomo: PomoSettings) -> Self {
        Self {
            media: None,
            timeline: None,
            lyrics_on: false,
            lyrics_ready: false,
            queue: VecDeque::new(),
            current: None,
            sticky: Vec::new(),
            fullscreen: false,
            hover: false,
            hover_blocked: false,
            visit_on_notice: false,
            arrange_pick: false,
            arrange_naming: false,
            reminder_seen: None,
            open_mode: OpenMode::Hover,
            latched: false,
            hover_opened: None,
            editor_open: false,
            cli_focused: false,
            cli_tall: false,
            cli_scroll: 0,
            cli_menu: false,
            clip_focused: false,
            notif_focused: false,
            notif_armed: None,
            hotkey_held: None,
            shelf_held: false,
            shelf_drop: DropVisit::No,
            search_open: false,
            search_prev: Page::Calendar,
            collapsed: Collapse::No,
            pomodoro: Pomodoro::new(pomo),
            page: Page::Calendar,
            pages: PanelPages::default(),
            tiles: ShortcutTiles::default(),
            wheel: 0,
            notes_page: 0,
            reminders_page: 0,
            day_page: 0,
            today,
            cal_view: CalView::Month,
            cal_anchor: today,
            cal_selected: today,
            cleaner: None,
            clean_busy: false,
            clean_mode: CleanMode::Own,
            mole: MolePhase::Unknown,
            mole_opts: MoleOpts::default(),
            mole_pending: false,
            shot_touched: false,
            linger_until: None,
            lingering: false,
        }
    }

    pub fn apply(&mut self, event: Event, now: Instant) -> Vec<Effect> {
        let calendar_was_shown = self.view() == View::Panel(Page::Calendar);
        let calendar_was = (self.cal_view, self.cal_anchor);
        let selected_was = self.cal_selected;
        let panel_was_open = self.panel_open();
        let cleaner_was = self.cleaner.map(|c| c.mode);
        let hover_on = matches!(event, Event::Hover(true));
        let page_was = self.page;
        // The linger runs out here, after the "was" values above saw the
        // panel as the user last did (the close below still cleans up after
        // it). From here on `panel_open_now` is the panel the event meets.
        self.lingering = self.linger_until.is_some_and(|until| until > now);
        if !self.lingering {
            self.linger_until = None;
        }
        let panel_open_now = self.panel_open();
        let panel_view_now = matches!(self.view(), View::Panel(_));
        let linger_trigger = self.linger_trigger(&event, now);
        let mut effects = Vec::new();
        // Finish a phase whose end has passed before handling the input, so a
        // click that beats the deadline's Tick never pauses the timer at 0:00.
        if let Some(done) = self.pomodoro.tick(now) {
            self.phase_done(done, &mut effects);
        }
        self.advance(now, &mut effects);
        self.expire_confirm(now);
        match event {
            Event::Media(media) => self.set_media(media, now),
            Event::MediaTimeline(timeline) => self.timeline = timeline,
            Event::LyricsReady(ready) => self.lyrics_ready = ready,
            Event::LyricsSetting(on) => self.lyrics_on = on,
            Event::Notice(notice) => match &notice.kind {
                NoticeKind::ClaudeWaiting { .. }
                | NoticeKind::Reminder { .. }
                | NoticeKind::MoleFailed => {
                    if let Some(key) = sticky_key(&notice) {
                        self.clear_sticky(key);
                    }
                    // The sticky slot is hidden in fullscreen, so show it briefly like
                    // any other new notice; it stays in `sticky` for afterwards.
                    if self.fullscreen {
                        self.queue.push_back(notice.clone());
                    }
                    self.sticky.push(notice);
                }
                NoticeKind::ClaudeDone { session_id } => {
                    self.clear_sticky(Sticky::Session(session_id));
                    self.queue.push_back(notice);
                }
                NoticeKind::App { .. }
                | NoticeKind::BatteryLow
                | NoticeKind::SystemLow
                | NoticeKind::CleanDone
                | NoticeKind::MoleDone
                | NoticeKind::ShotSaved
                | NoticeKind::ShotDeleted
                | NoticeKind::DevicePlugged { .. }
                | NoticeKind::DeviceSafe
                | NoticeKind::Arranged { .. }
                | NoticeKind::UpdateAvailable { .. }
                | NoticeKind::UpdateInfo
                | NoticeKind::Pomodoro => self.queue.push_back(notice),
                // The download's percent: one notice, replaced where it
                // stands (current or queued) so it never re-enters, and
                // timed from this update, not from the first.
                NoticeKind::UpdateProgress { .. } => {
                    if let Some((current, expires)) = &mut self.current
                        && is_update_progress(current)
                    {
                        *current = notice;
                        *expires = now + UPDATE_PROGRESS_DURATION;
                    } else if let Some(queued) = self.queue.iter_mut().find(|n| is_update_progress(n))
                    {
                        *queued = notice;
                    } else {
                        self.queue.push_back(notice);
                    }
                }
                // A failure ends the download's percent, shown or waiting.
                NoticeKind::UpdateFailed => {
                    self.queue.retain(|n| !is_update_progress(n));
                    if matches!(&self.current, Some((n, _)) if is_update_progress(n)) {
                        self.current = None;
                    }
                    self.queue.push_back(notice);
                }
                // A newer screenshot replaces an older preview and a
                // "Disalin" (both kept, nothing deleted); it never removes
                // another kind of notice and never shows over a game.
                NoticeKind::Screenshot { .. } => {
                    if !self.fullscreen {
                        let replaced = |n: &Notice| {
                            shot_of(n).is_some()
                                || n.kind == NoticeKind::Copied
                                || is_clip_preview(n)
                        };
                        self.queue.retain(|n| !replaced(n));
                        if matches!(&self.current, Some((n, _)) if replaced(n)) {
                            self.current = None;
                        }
                        self.queue.push_back(notice);
                    }
                }
                // Never over a fullscreen app (`App` checks first and tries
                // again later; this only guards a race).
                NoticeKind::Greeting { .. } => {
                    if !self.fullscreen {
                        self.queue.push_back(notice);
                    }
                }
                // Only the latest copy is worth saying; none over a game.
                NoticeKind::Copied => {
                    if !self.fullscreen {
                        self.queue.retain(|n| n.kind != NoticeKind::Copied);
                        if matches!(&self.current, Some((n, _)) if n.kind == NoticeKind::Copied) {
                            self.current = None;
                        }
                        self.queue.push_back(notice);
                    }
                }
                // The preview of a copy made elsewhere: like "Disalin" only
                // the latest is worth showing (it replaces an older preview
                // or a "Disalin"), and none over a game.
                NoticeKind::ClipPreview { .. } => {
                    if !self.fullscreen {
                        let replaced =
                            |n: &Notice| n.kind == NoticeKind::Copied || is_clip_preview(n);
                        self.queue.retain(|n| !replaced(n));
                        if matches!(&self.current, Some((n, _)) if replaced(n)) {
                            self.current = None;
                        }
                        self.queue.push_back(notice);
                    }
                }
            },
            Event::ClaudeActivity { session_id } => self.clear_sticky(Sticky::Session(&session_id)),
            Event::Fullscreen(on) => {
                self.fullscreen = on;
                if on {
                    self.latched = false;
                    self.end_linger();
                }
                // Hidden by a game: the preview goes, its file kept.
                if on {
                    self.queue.retain(|n| shot_of(n).is_none());
                    if matches!(&self.current, Some((n, _)) if shot_of(n).is_some()) {
                        self.current = None;
                    }
                    // A copy preview is stale by the time the game ends.
                    self.queue.retain(|n| !is_clip_preview(n));
                    if matches!(&self.current, Some((n, _)) if is_clip_preview(n)) {
                        self.current = None;
                    }
                }
            }
            Event::Hover(on) => {
                // The pointer left the notice it held: at least the grace more.
                let held = self
                    .current
                    .as_ref()
                    .is_some_and(|(n, _)| self.notice_held(n));
                if !on
                    && held
                    && let Some((_, expires)) = &mut self.current
                {
                    *expires = (*expires).max(now + SHOT_LEAVE_GRACE);
                }
                if on && !self.hover {
                    self.hover_blocked = !panel_open_now && self.notice_blocks_hover();
                    self.visit_on_notice = self.current.is_some();
                    // A sticky reminder keeps hover off for one visit only.
                    if self.hover_blocked && self.current.is_none() {
                        self.reminder_seen = self.sticky.last().cloned();
                    }
                }
                if !on {
                    self.hover_blocked = false;
                    self.visit_on_notice = false;
                }
                self.hover = on;
                // The drop's own visit ends when the pointer leaves it; a
                // later visit hands the panel to hover, as for any panel.
                match (self.shelf_drop, on) {
                    (DropVisit::Here, false) => self.shelf_drop = DropVisit::Left,
                    (DropVisit::Left, true) => {
                        self.shelf_drop = DropVisit::No;
                        self.latched = false;
                    }
                    _ => {}
                }
                // The pointer arrived: hover holds the hotkey's panel now,
                // and leaving closes it as usual.
                if on {
                    self.hotkey_held = None;
                    self.shelf_held = false;
                }
                self.collapsed = match (self.collapsed, on) {
                    (Collapse::Shut, false) => Collapse::Left,
                    // A new visit: the user came back for the panel.
                    (Collapse::Left, true) => Collapse::No,
                    (c, _) => c,
                };
            }
            Event::OpenMode(mode) => {
                self.open_mode = mode;
                self.latched = false;
                self.end_linger();
            }
            // A click outside the island: a panel kept open closes. Under the
            // pointer (an odd case) it stays shut until the pointer comes back.
            Event::Dismiss if self.arrange_pick => {}
            Event::Dismiss => {
                self.end_linger();
                if self.latched {
                    self.latched = false;
                    if self.hover {
                        self.collapse();
                    }
                }
            }
            Event::EditorOpen(on) => self.editor_open = on,
            Event::CliFocus(on) => self.cli_focused = on,
            Event::ClipFocus(on) => self.clip_focused = on,
            Event::NotifFocus(on) => self.notif_focused = on,
            Event::ClipHotkey => self.clip_hotkey(),
            Event::ClipHoldEnd => self.hotkey_held = None,
            Event::DevicesHotkey => self.devices_hotkey(),
            Event::ArrangeHotkey => self.page_hotkey(Page::Arrange),
            Event::ArrangePick(on) => self.arrange_pick = on,
            Event::ArrangeNaming(on) => self.arrange_naming = on,
            Event::PrintScreen => self.print_screen(&mut effects),
            Event::SearchHotkey => self.search_hotkey(&mut effects),
            Event::SearchOpen => {
                if self.search_open {
                    if !self.fullscreen {
                        effects.push(Effect::FocusSearch);
                    }
                } else {
                    self.search_hotkey(&mut effects);
                }
            }
            Event::SearchClose => self.close_search(),
            Event::ClipCommit => {
                if self.view() == View::Panel(Page::Clipboard) {
                    self.collapse();
                }
            }
            Event::CliScroll { lines, max } => {
                if self.view() == View::Panel(Page::Cli) {
                    let scrolled = self.cli_scroll as i64 + i64::from(lines);
                    self.cli_scroll = scrolled.clamp(0, max as i64) as usize;
                }
            }
            Event::CliOutput(n) => {
                // A view scrolled up stays on what it shows; at 0 it follows.
                if self.cli_scroll > 0 {
                    self.cli_scroll = self.cli_scroll.saturating_add(n);
                }
            }
            Event::Wheel(delta) => {
                // A wheel notch over the sticky reminder peeks at the panel
                // (its strip), and that notch is spent on opening it, not on
                // turning the page. Over a temporary notice the wheel is the
                // notice's: the panel stays behind it.
                if self.hover_blocked && self.current.is_none() && self.open_mode != OpenMode::Click
                {
                    self.hover_blocked = false;
                    self.wheel = 0;
                } else {
                    self.wheel(delta)
                }
            }
            Event::PageFlick { next } => {
                // The wheel's rules: the peek over the sticky reminder spends
                // the flick on opening the panel.
                if self.hover_blocked && self.current.is_none() && self.open_mode != OpenMode::Click
                {
                    self.hover_blocked = false;
                    self.wheel = 0;
                } else if self.can_turn_page() {
                    // A vertical residue must not add a second step.
                    self.wheel = 0;
                    self.turn_page(if next { 1 } else { -1 });
                }
            }
            Event::Today(date) => self.set_today(date),
            Event::ReminderCleared(id) => self.clear_sticky(Sticky::Reminder(&id)),
            Event::Click(hit) => self.click(hit, now, &mut effects),
            Event::Calendar(_)
            | Event::Toast(_)
            | Event::Update(_)
            | Event::Lyrics(_)
            | Event::Cli(_)
            | Event::ExplorerFolders { .. }
            | Event::SystemSample(_)
            | Event::Audio(..)
            | Event::Devices(_)
            | Event::Arrange(_)
            | Event::SearchIndex(_)
            | Event::SearchFiles(_)
            | Event::SearchDetail { .. }
            | Event::ShelfDrop(_)
            | Event::ShelfDragOver(_)
            | Event::Clean(_)
            | Event::Mole(_)
            | Event::ShotFolder { .. }
            | Event::ShotSaveDone { .. }
            | Event::ShotRecycled { .. }
            | Event::Tick => {}
            Event::CleanFinished => self.clean_busy = false,
            Event::CleanModeSetting(mode) => self.clean_mode = mode,
            Event::PanelPages(pages) => self.set_pages(pages),
            Event::ShortcutTiles(tiles) => self.tiles = tiles,
            // Files over the island: the panel opens on the shelf and stays
            // while they hover. In a fullscreen game the island is hidden, so
            // no drag reaches it (and `view` stays hidden if one did).
            Event::ShelfDrag(true) => {
                self.page = Page::Shelf;
                self.collapsed = Collapse::No;
                self.shelf_held = true;
            }
            // Never touches a drop's latch: the 600 ms leave timer and an
            // empty drop end only the drag's own hold.
            Event::ShelfDrag(false) => self.shelf_held = false,
            // Dropped and put on the shelf: the panel opens on it and stays,
            // latched like a click. In Hover mode the drop's own visit does
            // not end it (see `DropVisit`); HoverStay and Click already
            // keep a latched panel until a click outside.
            Event::ShelfDropped => {
                if !self.fullscreen {
                    self.page = Page::Shelf;
                    self.collapsed = Collapse::No;
                    self.shelf_held = false;
                    self.latched = true;
                    self.shelf_drop = if self.open_mode == OpenMode::Hover {
                        DropVisit::Here
                    } else {
                        DropVisit::No
                    };
                }
            }
            // The pointer left before the dwell, so no `Hover(false)` comes.
            Event::ShelfDropLeft => {
                if self.shelf_drop == DropVisit::Here {
                    self.shelf_drop = DropVisit::Left;
                }
            }
            // Only the open Mixer page takes a level.
            Event::MixerLevel(i, level) => {
                if self.view() == View::Panel(Page::Mixer) {
                    effects.push(Effect::MixerLevel(i, level));
                }
            }
            Event::MoleFound(found) => self.mole_found(found, &mut effects),
            Event::MolePreviewed { opts, ok } => {
                if self.mole == MolePhase::Scanning(opts) {
                    self.mole = MolePhase::Previewed { opts, ok };
                }
            }
            Event::MoleFinished { ok } => {
                if self.mole == MolePhase::Running {
                    self.mole = MolePhase::Finished { ok };
                }
            }
        }
        // A new notice or a dismissal may have left the slot free for the next queued one.
        self.advance(now, &mut effects);
        // The pointer over the preview: its expiry is no longer "ignored".
        if self.hover && self.shown_shot().is_some() {
            self.shot_touched = true;
        }
        // The search closed (or the panel went away some other way): the
        // page it came over returns, and the keyboard with it. Another page
        // taking its place (the Clipboard hotkey, files dragged over, a Mole
        // notice) closes it too and stays. First, so what follows sees the
        // panel as the search left it.
        if (self.search_open || self.page == Page::Search)
            && (self.page != Page::Search || !self.panel_open())
        {
            self.search_open = false;
            if self.page == Page::Search {
                self.page = self.search_prev;
            }
            effects.push(Effect::ReleaseSearchFocus);
        }
        // The input lets go once its page is gone: another page, or the
        // panel shut for a shortcut.
        if self.cli_focused && (self.page != Page::Cli || self.collapsed != Collapse::No) {
            self.cli_focused = false;
            effects.push(Effect::ReleaseCliFocus);
        }
        // The cleaner belongs to the System page of an open panel.
        if self.cleaner.is_some() && (self.page != Page::System || !self.panel_open()) {
            self.cleaner = None;
        }
        // A Mole notice was clicked: the cleaner opens on its log.
        if self.mole_pending && self.page == Page::System && self.panel_open() {
            self.mole_pending = false;
            self.open_mole_cleaner(&mut effects);
        }
        // Mole mode left: a preview stops, its result is thrown away.
        let mole_now = self.cleaner.map(|c| c.mode);
        if cleaner_was == Some(CleanMode::Mole) && mole_now != Some(CleanMode::Mole) {
            self.leave_mole(&mut effects);
        }
        if self.clip_focused && (self.page != Page::Clipboard || self.collapsed != Collapse::No) {
            self.clip_focused = false;
            effects.push(Effect::ReleaseClipFocus);
        }
        if self.notif_focused
            && (self.page != Page::Notifications || self.collapsed != Collapse::No)
        {
            self.notif_focused = false;
            effects.push(Effect::ReleaseNotifFocus);
        }
        // "Hapus semua" disarms with its page or the panel.
        if self.notif_armed.is_some() && (self.page != Page::Notifications || !self.panel_open()) {
            self.notif_armed = None;
        }
        if self
            .hotkey_held
            .is_some_and(|page| self.page != page || self.collapsed != Collapse::No)
        {
            self.hotkey_held = None;
        }
        if self.shelf_held && (self.page != Page::Shelf || self.collapsed != Collapse::No) {
            self.shelf_held = false;
        }
        // The menu belongs to the CLI page of an open panel.
        if self.cli_menu && (self.page != Page::Cli || !self.panel_open()) {
            self.cli_menu = false;
        }
        // Hover and stay: the panel the pointer opened stays until a click
        // outside, once it is what shows (not while a notice covers it).
        if self.hover_stay_latches() {
            self.latched = true;
        }
        // Whatever unlatched the panel (a click outside, a card, fullscreen,
        // a mode change) ends the drop's hold with it.
        if !self.latched {
            self.shelf_drop = DropVisit::No;
        }
        // Something the user did in the open panel keeps it open a while
        // longer. Here, around the event, not in `click`: only a panel that
        // was open (an editor closing may have been all that held it) and
        // still may be (a tile's collapse, a fullscreen app and Dismiss end
        // it) lingers.
        if linger_trigger.armed(
            page_was != self.page,
            panel_open_now,
            panel_view_now,
            self.panel_held(),
        ) && self.collapsed == Collapse::No
            && !self.fullscreen
        {
            self.linger_until = Some(now + ACTIVITY_LINGER);
            self.lingering = true;
        } else if linger_trigger == LingerTrigger::StillOpen && !self.panel_held() {
            // The input let go and only an earlier linger is left: it
            // closes at once, as Esc always did.
            self.end_linger();
        }
        // Whatever closed the panel ends it.
        if !self.panel_open() {
            self.end_linger();
        }
        if hover_on && !panel_open_now && self.panel_open() {
            self.hover_opened = Some(now);
        }
        // Closed, or no longer open by hover or a click (an editor or the
        // CLI input holds it after the pointer left): no guard. A click can
        // beat the hover dwell, so a latched panel keeps its guard.
        if !self.panel_open() || (!self.hover && !self.latched) {
            self.hover_opened = None;
        }
        if panel_was_open && !self.panel_open() {
            // A page that is off was opened by its button for that visit
            // only; the next hover starts on a page that is on.
            self.leave_hidden_page();
            self.wheel = 0;
            self.notes_page = 0;
            self.reminders_page = 0;
            self.cli_tall = false;
            self.cli_scroll = 0;
        }
        let calendar_shown = self.view() == View::Panel(Page::Calendar);
        // Any navigation, a view change, or the calendar leaving the screen
        // (panel closed or another page) starts the day list from the top.
        if calendar_was != (self.cal_view, self.cal_anchor)
            || selected_was != self.cal_selected
            || !calendar_shown
        {
            self.day_page = 0;
        }
        if (calendar_shown && !calendar_was_shown)
            || calendar_was != (self.cal_view, self.cal_anchor)
        {
            let (from, to) = self.cal_range();
            effects.push(Effect::RefreshCalendar { from, to });
        }
        effects
    }

    pub fn view(&self) -> View {
        if let Some((notice, _)) = &self.current {
            return View::Notice(notice.clone());
        }
        if self.fullscreen {
            return View::Hidden;
        }
        // Sticky notices do not block the panel; they show as a strip inside it.
        if self.panel_open() {
            return View::Panel(self.page);
        }
        if let Some(notice) = self.sticky.last() {
            return View::Notice(notice.clone());
        }
        if self.pomodoro.is_active() {
            return View::PomodoroCompact;
        }
        if let Some(media) = &self.media {
            if self.lyrics_on && self.lyrics_ready && media.playing {
                return View::MediaLyric;
            }
            return View::MediaCompact;
        }
        View::Idle
    }

    pub fn media(&self) -> Option<&MediaInfo> {
        self.media.as_ref()
    }

    /// A fullscreen app is in front (the island hides).
    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    pub fn timeline(&self) -> Option<&Timeline> {
        self.timeline.as_ref()
    }

    pub fn lyrics_on(&self) -> bool {
        self.lyrics_on
    }

    /// The CLI input has the keyboard and holds the panel open.
    pub fn cli_focused(&self) -> bool {
        self.cli_focused
    }

    /// The CLI page shows the enlarged (560 × 440) panel.
    pub fn cli_tall(&self) -> bool {
        self.cli_tall
    }

    /// Lines the CLI output is scrolled up from the newest.
    pub fn cli_scroll(&self) -> usize {
        self.cli_scroll
    }

    /// The CLI page's folder menu is open.
    pub fn cli_menu(&self) -> bool {
        self.cli_menu
    }

    /// The panel is open (hover, an editor or the CLI input holds it), even
    /// while a notice covers it.
    pub fn panel_is_open(&self) -> bool {
        self.panel_open()
    }

    pub fn lyrics_ready(&self) -> bool {
        self.lyrics_ready
    }

    /// A new track's lyrics are not known yet. When the same track pauses or
    /// resumes, the timeline is re-based at that moment, so a player that
    /// does not report a fresh one neither runs on while paused nor jumps
    /// ahead when it resumes (a fresh one from the watcher replaces it).
    fn set_media(&mut self, media: Option<MediaInfo>, now: Instant) {
        match (&self.media, &media) {
            (Some(old), Some(new)) if old.title == new.title && old.artist == new.artist => {
                if old.playing != new.playing
                    && let Some(t) = &self.timeline
                {
                    self.timeline = Some(lyrics::rebase(t, old.playing, now));
                }
            }
            // Another track, or none: its position is not this one's.
            _ => {
                self.lyrics_ready = false;
                self.timeline = None;
            }
        }
        self.media = media;
    }

    /// The earliest of the current notice's expiry and the pomodoro phase end;
    /// the caller should deliver a `Tick` then.
    pub fn next_deadline(&self) -> Option<Instant> {
        // A preview under the pointer has no deadline until it leaves.
        let notice = self
            .current
            .as_ref()
            .filter(|(n, _)| !self.notice_held(n))
            .map(|(_, expires)| *expires);
        let armed = self.cleaner.and_then(|c| c.armed_until);
        [
            notice,
            self.pomodoro.next_deadline(),
            armed,
            self.notif_armed,
            self.linger_until,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// The current notice waits for the pointer: it has a button and the
    /// pointer's visit began on it. One that arrived under the pointer (over
    /// an open panel) runs its time, so it never hides the panel for good.
    fn notice_held(&self, notice: &Notice) -> bool {
        (self.hover && self.visit_on_notice && notice_has_action(notice))
            || (self.arrange_naming && matches!(notice.kind, NoticeKind::Arranged { .. }))
    }

    /// Hover and stay latches the panel the pointer holds, once the panel is
    /// what shows: no notice over it, no notice keeping hover off, not shut.
    fn hover_stay_latches(&self) -> bool {
        self.open_mode == OpenMode::HoverStay
            && self.hover
            && !self.hover_blocked
            && self.current.is_none()
            && self.collapsed == Collapse::No
            && !self.fullscreen
    }

    /// The armed cleaner button (and "Hapus semua" of the Notifikasi page)
    /// turns back once `CLEAN_CONFIRM` has passed.
    fn expire_confirm(&mut self, now: Instant) {
        if self.notif_armed.is_some_and(|until| until <= now) {
            self.notif_armed = None;
        }
        if let Some(c) = &mut self.cleaner
            && c.armed_until.is_some_and(|until| until <= now)
        {
            c.armed_until = None;
        }
    }

    /// The cleaner is what the panel shows.
    fn cleaner_shown(&self) -> bool {
        self.cleaner.is_some() && self.view() == View::Panel(Page::System)
    }

    /// The cleaner shows in its own mode.
    fn own_shown(&self) -> bool {
        self.cleaner_shown() && self.cleaner.is_some_and(|c| c.mode == CleanMode::Own)
    }

    /// The cleaner shows in Mole mode.
    fn mole_shown(&self) -> bool {
        self.cleaner_shown() && self.cleaner.is_some_and(|c| c.mode == CleanMode::Mole)
    }

    /// The cleaner's button: the first click arms it, a second one before
    /// `CLEAN_CONFIRM` runs out asks for the clean. Nothing while a clean
    /// (either mode's) runs or nothing is ticked.
    fn clean_go(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        if !self.own_shown() || self.clean_busy || self.mole == MolePhase::Running {
            return;
        }
        let Some(c) = &mut self.cleaner else {
            return;
        };
        let picked = c.picked();
        if picked.is_empty() {
            c.armed_until = None;
            return;
        }
        match c.armed_until.take() {
            Some(until) if now < until => {
                self.clean_busy = true;
                effects.push(Effect::CleanDelete(picked));
            }
            _ => c.armed_until = Some(now + CLEAN_CONFIRM),
        }
    }

    /// "Jalankan Mole": only after a dry-run that finished well with the
    /// options ticked now, the first click arms it, a second one before
    /// `CLEAN_CONFIRM` runs out asks for the clean. After a run it starts a
    /// new preview ("Pratinjau lagi").
    fn mole_go(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        if !self.mole_shown() {
            return;
        }
        if matches!(self.mole, MolePhase::Finished { .. }) {
            self.start_preview(effects);
            return;
        }
        let ready = self.mole
            == MolePhase::Previewed {
                opts: self.mole_opts,
                ok: true,
            }
            && !self.clean_busy;
        let Some(c) = &mut self.cleaner else {
            return;
        };
        if !ready {
            c.armed_until = None;
            return;
        }
        match c.armed_until.take() {
            Some(until) if now < until => {
                c.mole_log = false;
                self.mole = MolePhase::Running;
                self.clear_sticky(Sticky::Mole);
                effects.push(Effect::MoleClean(self.mole_opts));
            }
            _ => c.armed_until = Some(now + CLEAN_CONFIRM),
        }
    }

    /// A new dry-run with the options ticked now; the old preview goes.
    fn start_preview(&mut self, effects: &mut Vec<Effect>) {
        self.mole = MolePhase::Scanning(self.mole_opts);
        if let Some(c) = &mut self.cleaner {
            c.armed_until = None;
            c.mole_log = false;
        }
        effects.push(Effect::MolePreview(self.mole_opts));
    }

    /// Mole mode came up: a clean that runs, or one that just ended when a
    /// notice asked for its log, shows as it is; otherwise Mole is looked
    /// for and, once found, previewed.
    fn enter_mole(&mut self, for_log: bool, effects: &mut Vec<Effect>) {
        match self.mole {
            MolePhase::Running => {}
            MolePhase::Finished { .. } if for_log => {}
            _ => {
                self.mole = MolePhase::Detecting;
                effects.push(Effect::MoleDetect);
            }
        }
    }

    /// Mole mode went (another mode, or the cleaner closed): a dry-run
    /// stops and every preview is forgotten. A clean goes on.
    fn leave_mole(&mut self, effects: &mut Vec<Effect>) {
        match self.mole {
            MolePhase::Scanning(_) => {
                effects.push(Effect::MoleStopPreview);
                self.mole = MolePhase::Unknown;
            }
            MolePhase::Detecting | MolePhase::Missing | MolePhase::Previewed { .. } => {
                self.mole = MolePhase::Unknown;
            }
            MolePhase::Unknown | MolePhase::Running | MolePhase::Finished { .. } => {}
        }
    }

    fn mole_found(&mut self, found: bool, effects: &mut Vec<Effect>) {
        if self.mole != MolePhase::Detecting {
            return;
        }
        if !found {
            self.mole = MolePhase::Missing;
        } else if self.cleaner.is_some_and(|c| c.mode == CleanMode::Mole) {
            self.start_preview(effects);
        } else {
            self.mole = MolePhase::Unknown;
        }
    }

    /// The segment: the other mode, remembered. Not while a clean runs.
    fn switch_mode(&mut self, mode: CleanMode, effects: &mut Vec<Effect>) {
        if !self.cleaner_shown() || self.clean_busy || self.mole == MolePhase::Running {
            return;
        }
        let Some(c) = &mut self.cleaner else {
            return;
        };
        if c.mode == mode {
            return;
        }
        c.mode = mode;
        c.armed_until = None;
        c.mole_log = false;
        self.clean_mode = mode;
        effects.push(Effect::SaveCleanMode(mode));
        match mode {
            CleanMode::Mole => self.enter_mole(false, effects),
            CleanMode::Own => effects.push(Effect::OpenCleaner),
        }
    }

    /// A Mole notice was clicked: the panel shows the cleaner on Mole's
    /// log the next time it opens on the System page (now, from the strip).
    fn open_mole_log(&mut self) {
        self.page = Page::System;
        self.mole_pending = true;
    }

    fn open_mole_cleaner(&mut self, effects: &mut Vec<Effect>) {
        match &mut self.cleaner {
            Some(c) if c.mode == CleanMode::Mole => return,
            Some(c) => {
                c.mode = CleanMode::Mole;
                c.armed_until = None;
            }
            None => self.cleaner = Some(CleanerUi::new(CleanMode::Mole)),
        }
        self.enter_mole(true, effects);
    }

    fn panel_open(&self) -> bool {
        self.panel_held() || (self.collapsed == Collapse::No && self.lingering)
    }

    /// The panel is open for something other than the linger.
    fn panel_held(&self) -> bool {
        let pointer = self.hover && !self.hover_blocked && self.open_mode != OpenMode::Click;
        self.collapsed == Collapse::No
            && (pointer
                || self.latched
                || self.editor_open
                || self.cli_focused
                || self.clip_focused
                || self.notif_focused
                || self.hotkey_held.is_some()
                || self.shelf_held
                || self.arrange_pick
                || self.search_open)
    }

    /// What, in `event`, may keep the open panel a while longer.
    fn linger_trigger(&self, event: &Event, now: Instant) -> LingerTrigger {
        match event {
            // An editor closed, saved, deleted or cancelled: the panel it
            // held would snap shut with the pointer away.
            Event::EditorOpen(false) => LingerTrigger::Always,
            // A click on the panel's content. A media button the hover guard
            // drops did nothing.
            Event::Click(Hit::PanelPage(_)) => LingerTrigger::PageTurn,
            Event::Click(hit) if !(is_media_hit(hit) && self.media_guarded(now)) => {
                LingerTrigger::Panel
            }
            // A page turned by the wheel (a notch that did not turn one
            // arms nothing).
            Event::Wheel(_) | Event::PageFlick { .. } => LingerTrigger::PageTurn,
            Event::CliFocus(false) | Event::ClipFocus(false) | Event::NotifFocus(false) => {
                LingerTrigger::StillOpen
            }
            _ => LingerTrigger::No,
        }
    }

    /// Ends the linger (a tile's collapse, a click outside, a fullscreen app,
    /// a mode change, the panel closing).
    fn end_linger(&mut self) {
        self.linger_until = None;
        self.lingering = false;
    }

    /// The panel gets out of the way (a shortcut, a copy with Enter): shut
    /// whatever holds it until the pointer leaves and comes back. Without
    /// hover the pointer is already away: its next visit reopens it.
    fn collapse(&mut self) {
        self.latched = false;
        self.end_linger();
        self.collapsed = if self.hover {
            Collapse::Shut
        } else {
            Collapse::Left
        };
        self.hotkey_held = None;
        self.shelf_held = false;
    }

    /// The Clipboard hotkey: the Clipboard page opens (a new visit, whatever
    /// closed the panel before), or, when it shows already, the panel closes.
    fn clip_hotkey(&mut self) {
        self.page_hotkey(Page::Clipboard);
    }

    /// The Devices hotkey: the same for the Devices page.
    fn devices_hotkey(&mut self) {
        self.page_hotkey(Page::Devices);
    }

    /// A page's hotkey: it opens `page` as a new visit held open until the
    /// pointer comes, or, when `page` shows already, closes the panel.
    fn page_hotkey(&mut self, page: Page) {
        if self.fullscreen {
            return;
        }
        if self.view() == View::Panel(page) {
            if self.hover
                || self.editor_open
                || self.cli_focused
                || self.clip_focused
                || self.notif_focused
                || self.lingering
            {
                self.collapse();
            }
            self.hotkey_held = None;
            return;
        }
        self.page = page;
        self.collapsed = Collapse::No;
        // Under the pointer already: hover holds it, as for any visit.
        self.hotkey_held = (!self.hover).then_some(page);
    }

    /// The quick search's hotkey: it opens over whatever page shows (a new
    /// visit, whatever closed the panel before) and asks for the keyboard;
    /// pressed again, it closes. The CLI input or the Clipboard search field
    /// may have the keyboard: it passes straight to the search's field, so
    /// theirs is let go without handing it back to the window before (that
    /// would pull it from the search at once), and that window stays the one
    /// the search returns to.
    fn search_hotkey(&mut self, effects: &mut Vec<Effect>) {
        if self.fullscreen {
            return;
        }
        if self.search_open {
            self.search_open = false;
            self.collapse();
            return;
        }
        self.cli_focused = false;
        self.clip_focused = false;
        self.notif_focused = false;
        if self.page != Page::Search {
            self.search_prev = self.page;
        }
        self.page = Page::Search;
        self.collapsed = Collapse::No;
        self.search_open = true;
        effects.push(Effect::FocusSearch);
    }

    /// What a hover would land on is a notice with a button: the panel stays
    /// shut for this visit.
    fn notice_blocks_hover(&self) -> bool {
        if self.fullscreen {
            return false;
        }
        match &self.current {
            Some((n, _)) => notice_has_action(n),
            // The newest sticky one is what shows: only a reminder has the
            // Selesai button, and it keeps hover off for the first visit
            // after it appeared; later visits open the panel with it in the
            // strip (as in v3.1).
            None => self.sticky.last().is_some_and(|n| {
                matches!(n.kind, NoticeKind::Reminder { .. })
                    && self.reminder_seen.as_ref() != Some(n)
            }),
        }
    }

    /// The panel stays open until a click outside the island: `App` watches
    /// the mouse buttons only then, and sends `Event::Dismiss`.
    pub fn outside_click_wanted(&self) -> bool {
        (self.latched || self.lingering) && self.panel_open() && !self.fullscreen
    }

    /// "Mode buka": a click on the compact island opens the panel and keeps it.
    fn open_by_click(&mut self, now: Instant) {
        self.latched = true;
        self.collapsed = Collapse::No;
        self.hover_opened = Some(now);
    }

    /// A drop is holding the panel and its own visit is not over: a leave
    /// that sends no `Hover(false)` (the dwell never fired) must say so with
    /// `ShelfDropLeft`.
    pub fn shelf_drop_here(&self) -> bool {
        self.shelf_drop == DropVisit::Here
    }

    /// The Clipboard search field has the keyboard.
    pub fn clip_focused(&self) -> bool {
        self.clip_focused
    }

    /// The Notifikasi page's hidden input has the keyboard.
    pub fn notif_focused(&self) -> bool {
        self.notif_focused
    }

    /// "Hapus semua" waits for its second click.
    pub fn notif_armed(&self) -> bool {
        self.notif_armed.is_some()
    }

    /// The quick search is open.
    pub fn search_open(&self) -> bool {
        self.search_open
    }

    /// A page's hotkey (the Clipboard's or the Devices') holds the panel
    /// open.
    pub fn hotkey_held(&self) -> bool {
        self.hotkey_held.is_some()
    }

    /// The Clipboard hotkey holds the panel open.
    #[cfg(test)]
    pub fn clip_held(&self) -> bool {
        self.hotkey_held == Some(Page::Clipboard)
    }

    /// The Devices hotkey holds the panel open.
    #[cfg(test)]
    pub fn devices_held(&self) -> bool {
        self.hotkey_held == Some(Page::Devices)
    }

    /// How long ago hover opened the panel; `None` when it is closed or held
    /// open by something else (an editor, the CLI input).
    pub fn hover_open_age(&self, now: Instant) -> Option<Duration> {
        self.hover_opened
            .map(|at| now.saturating_duration_since(at))
    }

    /// The panel opened by hover less than `MEDIA_HOVER_GUARD` ago: its media
    /// buttons do nothing yet.
    pub fn media_guarded(&self, now: Instant) -> bool {
        self.hover_open_age(now)
            .is_some_and(|age| age < MEDIA_HOVER_GUARD)
    }

    fn click(&mut self, hit: Hit, now: Instant, effects: &mut Vec<Effect>) {
        if is_media_hit(&hit) && self.media_guarded(now) {
            return;
        }
        // Any click but the button's own disarms it.
        if !matches!(hit, Hit::CleanGo | Hit::MoleGo)
            && let Some(c) = &mut self.cleaner
        {
            c.armed_until = None;
        }
        // Any click but its own disarms "Hapus semua".
        if !matches!(hit, Hit::NotifClear) {
            self.notif_armed = None;
        }
        match hit {
            // Click mode: the compact island opens the panel (a ready
            // pomodoro starts from inside it).
            // A sticky notice (Claude waiting, a due reminder) is what the
            // closed island shows: the click opens the panel, whose strip
            // keeps the notice and its own click.
            Hit::Body
                if self.open_mode == OpenMode::Click
                    && (matches!(
                        self.view(),
                        View::Idle | View::MediaCompact | View::MediaLyric | View::PomodoroCompact
                    ) || (self.current.is_none()
                        && matches!(self.view(), View::Notice(_)))) =>
            {
                self.open_by_click(now)
            }
            Hit::Body => match self.view() {
                View::Notice(n) => {
                    // These notices lead to the panel: it opens under the
                    // pointer after the click, and in the modes that keep it
                    // open, it stays (the pointer alone would not hold it).
                    // The copy preview only when the Clipboard page is on in
                    // "Halaman panel"; off, the click just dismisses it.
                    let opens_panel = matches!(
                        n.kind,
                        NoticeKind::SystemLow | NoticeKind::MoleDone | NoticeKind::MoleFailed
                    ) || (is_clip_preview(&n)
                        && self.pages.is_on(Page::Clipboard));
                    effects.extend(self.dismiss_visible_notice(now));
                    // A plugged-in drive's "Buka": Explorer on its first
                    // drive, and the panel stays out of the way like after a
                    // tile.
                    if let NoticeKind::DevicePlugged { letter } = n.kind
                        && letter.is_ascii_alphabetic()
                    {
                        self.collapse();
                        effects.push(Effect::OpenDrive(letter));
                    }
                    // The update notice's chips: install, or the releases page.
                    match &n.kind {
                        NoticeKind::UpdateAvailable { .. } => effects.push(Effect::InstallUpdate),
                        NoticeKind::UpdateFailed => effects.push(Effect::OpenReleases),
                        _ => {}
                    }
                    if opens_panel {
                        self.hover_blocked = false;
                        if self.open_mode != OpenMode::Hover && !self.fullscreen {
                            self.open_by_click(now);
                        }
                    }
                }
                View::PomodoroCompact if self.pomodoro.status() == PomoStatus::Ready => {
                    self.pomodoro.start_pause(now)
                }
                _ => {}
            },
            Hit::Prev => effects.push(Effect::Media(MediaCommand::Prev)),
            // What the island shows decides: playing pauses, paused plays.
            Hit::PlayPause => {
                if let Some(media) = &self.media {
                    let cmd = if media.playing {
                        MediaCommand::Pause
                    } else {
                        MediaCommand::Play
                    };
                    effects.push(Effect::Media(cmd));
                }
            }
            Hit::Next => effects.push(Effect::Media(MediaCommand::Next)),
            Hit::PomoStartPause => self.pomodoro.start_pause(now),
            Hit::PomoReset => self.pomodoro.reset(),
            Hit::PomoSkip => {
                let done = self.pomodoro.skip(now);
                self.phase_done(done, effects);
            }
            Hit::PomoAdjust(field, dir) => {
                if self.pomodoro.adjust(field, dir) {
                    effects.push(Effect::SavePomodoro(self.pomodoro.settings()));
                }
            }
            Hit::CalPrev => self.move_anchor(false),
            Hit::CalNext => self.move_anchor(true),
            Hit::CalViewSel(view) => {
                self.cal_view = view;
                self.cal_anchor = self.cal_selected;
            }
            Hit::CalDate(date) => {
                self.cal_selected = date;
                if self.cal_view == CalView::Day {
                    self.cal_anchor = date;
                }
            }
            // In the day view on that date it pages through the day's events
            // (the layout wraps the count); elsewhere it opens that day.
            Hit::CalMore(date)
                if self.cal_view == CalView::Day
                    && self.cal_anchor == date
                    && self.cal_selected == date =>
            {
                self.day_page += 1
            }
            Hit::CalMore(date) => {
                self.cal_view = CalView::Day;
                self.cal_anchor = date;
                self.cal_selected = date;
            }
            Hit::CalAdd => effects.push(Effect::OpenEventEditor {
                event_id: None,
                date: self.cal_selected,
            }),
            Hit::CalEvent(id) => effects.push(Effect::OpenEventEditor {
                event_id: Some(id),
                date: self.cal_selected,
            }),
            Hit::CalConnect => effects.push(Effect::ConnectGoogle),
            Hit::NoteNew => effects.push(Effect::OpenNoteEditor(None)),
            Hit::NoteOpen(id) => effects.push(Effect::OpenNoteEditor(Some(id))),
            Hit::NotesMore => self.notes_page += 1,
            Hit::ReminderNew => effects.push(Effect::OpenReminderEditor(None)),
            Hit::ReminderOpen(id) => effects.push(Effect::OpenReminderEditor(Some(id))),
            Hit::RemindersMore => self.reminders_page += 1,
            Hit::LyricsToggle => {
                self.lyrics_on = !self.lyrics_on;
                effects.push(Effect::SaveLyrics(self.lyrics_on));
            }
            // The slider is set by the press itself (`Event::MixerLevel`); the
            // click that ends it has nothing left to do.
            Hit::MixerSlider(_) => {}
            Hit::MixerMute(i) => {
                if self.view() == View::Panel(Page::Mixer) {
                    effects.push(Effect::MixerMute(i));
                }
            }
            // A card opens its file, with the panel out of the way; the
            // others change the shelf where it is.
            Hit::ShelfCard(i) => {
                if self.view() == View::Panel(Page::Shelf) {
                    self.collapse_for(Effect::ShelfOpen(i), effects);
                }
            }
            Hit::ShelfRemove(i) => {
                if self.view() == View::Panel(Page::Shelf) {
                    effects.push(Effect::ShelfRemove(i));
                }
            }
            Hit::ShelfClear => {
                if self.view() == View::Panel(Page::Shelf) {
                    effects.push(Effect::ShelfClear);
                }
            }
            // The Devices page's buttons count only while it shows. A device
            // that is opened gets the panel out of the way first.
            Hit::DeviceEject(i) | Hit::DeviceRetry(i) => {
                if self.view() == View::Panel(Page::Devices) {
                    effects.push(Effect::DeviceEject(i));
                }
            }
            Hit::DeviceDetail(i) => {
                if self.view() == View::Panel(Page::Devices) {
                    effects.push(Effect::DeviceDetail(i));
                }
            }
            Hit::DevicesEjectAll => {
                if self.view() == View::Panel(Page::Devices) {
                    effects.push(Effect::DevicesEjectAll);
                }
            }
            Hit::DeviceOpen(i) => {
                if self.view() == View::Panel(Page::Devices) {
                    self.collapse_for(Effect::DeviceOpen(i), effects);
                }
            }
            // Susun's controls count only while its page shows.
            Hit::ArrangeMonitor(i) => {
                if self.view() == View::Panel(Page::Arrange) {
                    effects.push(Effect::ArrangeMonitor(i));
                }
            }
            Hit::ArrangeCard(i) => {
                if self.view() == View::Panel(Page::Arrange) {
                    effects.push(Effect::ArrangeCard(i));
                }
            }
            Hit::ArrangePlay(i) => {
                if self.view() == View::Panel(Page::Arrange) {
                    effects.push(Effect::ArrangePlay(i));
                }
            }
            Hit::ArrangeWindow(i) => {
                if self.view() == View::Panel(Page::Arrange) {
                    effects.push(Effect::ArrangeWindow(i));
                }
            }
            Hit::ArrangeSkip => {
                if self.view() == View::Panel(Page::Arrange) {
                    effects.push(Effect::ArrangeSkip);
                }
            }
            // The Arranged notice's buttons: "Batalkan" closes the notice
            // (App runs the undo); "Simpan…" keeps it for the name field.
            Hit::ArrangeUndo => {
                if matches!(self.view(), View::Notice(n) if matches!(n.kind, NoticeKind::Arranged { .. }))
                {
                    effects.push(Effect::ArrangeUndo);
                    effects.extend(self.dismiss_visible_notice(now));
                }
            }
            Hit::ArrangeSave => {
                if matches!(self.view(), View::Notice(n) if matches!(n.kind, NoticeKind::Arranged { save: ArrangeSave::Offer }))
                {
                    effects.push(Effect::ArrangeSave);
                }
            }
            // The strip shows the newest sticky notice; clicking it dismisses that one.
            Hit::Strip => {
                if matches!(self.view(), View::Panel(_)) {
                    effects.extend(self.dismiss_newest_sticky());
                }
            }
            // The island's own quick search, the way its hotkey opens it.
            Hit::Shortcut(Shortcut::Search) => {
                if matches!(self.view(), View::Panel(_)) {
                    self.search_hotkey(effects);
                }
            }
            // The island's own history, not Win+V: its page opens.
            Hit::Shortcut(Shortcut::Clipboard) => {
                if matches!(self.view(), View::Panel(_)) {
                    self.page = Page::Clipboard;
                }
            }
            // A dock icon (the hovered page dots) turns to that page.
            Hit::PanelPage(page) => self.go_to_page(page),
            // A page tile jumps there, like the Clipboard tile: no collapse,
            // and a page that is switched off opens too (it holds for this
            // visit only, `leave_hidden_page` moves off it on close).
            Hit::Shortcut(Shortcut::Go(page)) => {
                if matches!(self.view(), View::Panel(_)) {
                    self.page = page;
                }
            }
            // Vale's own Settings window; the panel gets out of the way.
            Hit::Shortcut(Shortcut::ValeSettings) => {
                self.collapse_for(Effect::OpenSettings, effects)
            }
            // The panel gets out of the way first: the screenshot or picker
            // is for the window underneath. `App` waits for that.
            Hit::Shortcut(s) => self.collapse_for(Effect::RunShortcut(s), effects),
            // The back button: back to the page the search came over, with
            // the panel latched open (Esc closes the panel instead).
            Hit::SearchBack => self.search_back(now, effects),
            // Runs it and closes the search (Enter is this click too).
            Hit::SearchResult(i) => {
                if self.search_open {
                    self.search_open = false;
                    self.collapse();
                    effects.push(Effect::SearchRun(i));
                }
            }
            Hit::CustomTile(i) => {
                if let Some(Tile::Custom(c)) = self.tiles.visible().get(i) {
                    self.collapse_for(Effect::RunCustom(c.clone()), effects);
                }
            }
            // A process row opens Task Manager the way its tile does; a
            // drive opens Explorer there. Both get the panel out of the way.
            Hit::SystemProcess => {
                self.collapse_for(Effect::RunShortcut(Shortcut::TaskManager), effects)
            }
            Hit::SystemDrive(letter) => self.collapse_for(Effect::OpenDrive(letter), effects),
            Hit::SystemClean => {
                if self.view() == View::Panel(Page::System) && self.cleaner.is_none() {
                    // A Mole clean that runs is what the cleaner shows.
                    let mode = if self.mole == MolePhase::Running {
                        CleanMode::Mole
                    } else {
                        self.clean_mode
                    };
                    self.cleaner = Some(CleanerUi::new(mode));
                    match mode {
                        CleanMode::Own => effects.push(Effect::OpenCleaner),
                        CleanMode::Mole => self.enter_mole(false, effects),
                    }
                }
            }
            Hit::CleanMode(mode) => self.switch_mode(mode, effects),
            Hit::MoleOpt(opt) => {
                // Not on the install card, not while a clean runs.
                if self.mole_shown()
                    && !matches!(self.mole, MolePhase::Running | MolePhase::Missing)
                {
                    self.mole_opts = self.mole_opts.toggled(opt);
                    if matches!(
                        self.mole,
                        MolePhase::Scanning(_)
                            | MolePhase::Previewed { .. }
                            | MolePhase::Finished { .. }
                    ) {
                        self.start_preview(effects);
                    }
                }
            }
            Hit::MoleGo => self.mole_go(now, effects),
            Hit::MoleProtect => {
                if self.mole_shown()
                    && matches!(
                        self.mole,
                        MolePhase::Scanning(_)
                            | MolePhase::Previewed { .. }
                            | MolePhase::Finished { .. }
                    )
                {
                    effects.push(Effect::MoleWhitelist);
                }
            }
            Hit::MoleInstall => {
                // The panel gets out of the way of the browser.
                if self.mole_shown() && self.mole == MolePhase::Missing {
                    self.collapse_for(Effect::OpenMoleInstall, effects);
                }
            }
            Hit::MoleShowLog => {
                if self.mole_shown()
                    && matches!(self.mole, MolePhase::Previewed { .. })
                    && let Some(c) = &mut self.cleaner
                {
                    c.mole_log = !c.mole_log;
                }
            }
            Hit::CleanBack => {
                if self.cleaner_shown() {
                    self.cleaner = None;
                }
            }
            Hit::CleanToggle(cat) => {
                if self.own_shown()
                    && !self.clean_busy
                    && let Some(c) = &mut self.cleaner
                {
                    c.checked[cat.index()] = !c.checked[cat.index()];
                }
            }
            Hit::CleanGo => self.clean_go(now, effects),
            // The Notifikasi page's controls count only while it shows.
            Hit::NotifRow(_)
            | Hit::NotifOpen(_)
            | Hit::NotifMute(_)
            | Hit::NotifRemove(_)
            | Hit::NotifPause
            | Hit::NotifClear
            | Hit::NotifFocus
            | Hit::NotifSettings
                if self.view() != View::Panel(Page::Notifications) => {}
            // A row and its "Buka" both open the app and mark it read.
            Hit::NotifRow(id) | Hit::NotifOpen(id) => {
                effects.push(Effect::Notif(NotifAction::Open(id)))
            }
            Hit::NotifMute(id) => effects.push(Effect::Notif(NotifAction::Mute(id))),
            Hit::NotifRemove(id) => effects.push(Effect::Notif(NotifAction::Remove(id))),
            Hit::NotifPause => effects.push(Effect::Notif(NotifAction::TogglePause)),
            Hit::NotifSettings => effects.push(Effect::Notif(NotifAction::OpenSettings)),
            // The first click arms; a second within `CLEAN_CONFIRM` clears.
            Hit::NotifClear => match self.notif_armed.take() {
                Some(until) if now < until => {
                    effects.push(Effect::Notif(NotifAction::ClearAll));
                }
                _ => self.notif_armed = Some(now + CLEAN_CONFIRM),
            },
            // The list's background or the title: the page takes the
            // keyboard (the island activates, as for the CLI input).
            Hit::NotifFocus => effects.push(Effect::FocusNotifInput),
            Hit::ClipSearch
            | Hit::ClipCard(_)
            | Hit::ClipPin(_)
            | Hit::ClipPlain(_)
            | Hit::ClipDelete(_)
                if self.view() != View::Panel(Page::Clipboard) => {}
            Hit::ClipSearch => effects.push(Effect::FocusClipSearch),
            Hit::ClipCard(id) => effects.push(Effect::Clip(ClipAction::Copy(id))),
            Hit::ClipPin(id) => effects.push(Effect::Clip(ClipAction::TogglePin(id))),
            Hit::ClipPlain(id) => effects.push(Effect::Clip(ClipAction::CopyPlain(id))),
            Hit::ClipDelete(id) => effects.push(Effect::Clip(ClipAction::Delete(id))),
            Hit::ShotCopy(id) => self.shot_button(id, ShotAction::Copy(id), effects),
            Hit::ShotKeep(id) => self.shot_button(id, ShotAction::Keep(id), effects),
            Hit::ShotDelete(id) => self.shot_button(id, ShotAction::Delete(id), effects),
            // The preview stays; opening the file counts as a visit.
            Hit::ShotOpen(id) => {
                if self.shown_shot() == Some(id) {
                    self.shot_touched = true;
                    effects.push(Effect::Shot(ShotAction::Open(id)));
                }
            }
            Hit::CliInput
            | Hit::CliStop
            | Hit::CliExpand
            | Hit::CliFolder
            | Hit::CliFolderPick { .. }
            | Hit::CliFolderStar { .. }
            | Hit::CliMenuDismiss
                if self.view() != View::Panel(Page::Cli) =>
            {
                self.cli_menu = false;
            }
            Hit::CliInput => effects.push(Effect::FocusCliInput),
            Hit::CliStop => effects.push(Effect::StopCli),
            Hit::CliExpand => self.cli_tall = !self.cli_tall,
            // The menu is drawn in the panel: none of these touch the
            // keyboard (no FocusCliInput, no ReleaseCliFocus).
            Hit::CliFolder => self.cli_menu = !self.cli_menu,
            Hit::CliFolderPick { path, .. } => {
                if std::mem::take(&mut self.cli_menu) {
                    effects.push(Effect::CliChangeFolder(path));
                }
            }
            Hit::CliFolderStar { path, on } => {
                if self.cli_menu {
                    effects.push(Effect::CliFavorite { path, on });
                }
            }
            Hit::CliMenuDismiss => self.cli_menu = false,
        }
    }

    /// The PrtSc key: the same as the Screenshot tile with Vale's own tool.
    /// An open panel (or the search) shuts first, so it is not in the
    /// picture; with no panel the capture is asked for at once. `App` waits
    /// out the collapse before it starts.
    fn print_screen(&mut self, effects: &mut Vec<Effect>) {
        // Not `view()`: a notice on top hides a panel that is still held
        // under it and would come back during the capture.
        if self.panel_open() || self.search_open {
            self.collapse();
        }
        effects.push(Effect::RunCapture);
    }

    /// Shuts the open panel and asks for `effect` (a shortcut, Explorer on
    /// a drive), which `App` runs once it is out of the way. Nothing while
    /// the panel is not showing.
    fn collapse_for(&mut self, effect: Effect, effects: &mut Vec<Effect>) {
        if matches!(self.view(), View::Panel(_)) {
            // Without hover (an editor or the CLI pins the panel) the
            // pointer is already away: its next visit reopens it.
            self.collapse();
            effects.push(effect);
        }
    }

    /// Scrolling down (negative delta) moves right through the pages. Steps
    /// past either end are spent, so one notch back always moves a page.
    fn wheel(&mut self, delta: i32) {
        if !self.can_turn_page() {
            return;
        }
        self.wheel = self.wheel.saturating_add(delta);
        let steps = self.wheel / WHEEL_STEP;
        self.wheel -= steps * WHEEL_STEP;
        // Steps count up the wheel; later pages are down it.
        self.turn_page(-i64::from(steps));
    }

    /// Whether the wheel or a flick may turn a page: the panel shows one,
    /// and it is not the search (not in the order of pages).
    fn can_turn_page(&self) -> bool {
        matches!(self.view(), View::Panel(_)) && self.page != Page::Search
    }

    /// Closes the quick search (Esc): the panel collapses and the page it
    /// came over returns in the post-process of `apply`. The back button is
    /// `search_back`, which keeps the panel open.
    fn close_search(&mut self) {
        if self.search_open {
            self.search_open = false;
            self.collapse();
        }
    }

    /// The search's back button: the page it came over shows again and the
    /// panel stays open on it, latched like a click-open (a click outside
    /// closes it). With the notch closed when the search opened, that page is
    /// the last one the panel showed. The keyboard goes back here: the
    /// post-process of `apply` only releases it for a search still open or
    /// still showing, and this one is neither, so it sends nothing twice.
    fn search_back(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        if !self.search_open {
            return;
        }
        self.search_open = false;
        self.page = back_page(self.search_prev);
        self.open_by_click(now);
        effects.push(Effect::ReleaseSearchFocus);
    }

    /// A click on a dock icon: that page, with the flick's rules. The peek
    /// over the sticky reminder spends the click on opening the panel; the
    /// page the panel shows already does nothing. The slide's direction is
    /// the pages' places in the owner's order (`App`).
    fn go_to_page(&mut self, target: Page) {
        if self.hover_blocked && self.current.is_none() && self.open_mode != OpenMode::Click {
            self.hover_blocked = false;
            self.wheel = 0;
        } else if self.can_turn_page() && target != self.page {
            // A vertical residue must not add a second step.
            self.wheel = 0;
            self.show_page(target);
        }
    }

    /// Moves `steps` pages through the owner's order (negative: back).
    fn turn_page(&mut self, steps: i64) {
        self.show_page(self.pages.step(self.page, steps));
    }

    fn show_page(&mut self, to: Page) {
        let from = self.page;
        self.page = to;
        // A page turned with the pointer on a dropped shelf: the panel is
        // the pointer's now (and closes when it leaves). Without hover the
        // dwell has not fired yet, so the latch stays for it.
        if self.page != from && self.hover && self.shelf_drop != DropVisit::No {
            self.shelf_drop = DropVisit::No;
            self.latched = false;
        }
    }

    /// "Halaman panel" changed: a page just turned off is left for the next
    /// shown one (or the one before it at the end of the order).
    fn set_pages(&mut self, pages: PanelPages) {
        let was_on = self.pages.is_on(self.page);
        self.pages = pages;
        if was_on {
            self.leave_hidden_page();
        }
    }

    /// A page that is off hands over to the next page on (or the one before
    /// it at the end of the order).
    fn leave_hidden_page(&mut self) {
        if self.pages.is_on(self.page) {
            return;
        }
        let next = self.pages.step(self.page, 1);
        self.page = if next == self.page {
            self.pages.step(self.page, -1)
        } else {
            next
        };
    }

    /// Day view: one day, and the selection follows. Week: 7 days. Month: one
    /// calendar month, clamped to the month's last day.
    fn move_anchor(&mut self, forward: bool) {
        let a = self.cal_anchor;
        let moved = match (self.cal_view, forward) {
            (CalView::Day, true) => a.checked_add_days(Days::new(1)),
            (CalView::Day, false) => a.checked_sub_days(Days::new(1)),
            (CalView::Week, true) => a.checked_add_days(Days::new(7)),
            (CalView::Week, false) => a.checked_sub_days(Days::new(7)),
            (CalView::Month, true) => a.checked_add_months(Months::new(1)),
            (CalView::Month, false) => a.checked_sub_months(Months::new(1)),
        };
        self.cal_anchor = moved.unwrap_or(a);
        if self.cal_view == CalView::Day {
            self.cal_selected = self.cal_anchor;
        }
    }

    /// Dates still pointing at the old today follow it to the new one.
    fn set_today(&mut self, date: NaiveDate) {
        if self.cal_anchor == self.today {
            self.cal_anchor = date;
        }
        if self.cal_selected == self.today {
            self.cal_selected = date;
        }
        self.today = date;
    }

    /// Announces a finished phase: one sound and a notice that replaces any
    /// earlier pomodoro notice still queued or showing.
    fn phase_done(&mut self, done: PhaseDone, effects: &mut Vec<Effect>) {
        let settings = self.pomodoro.settings();
        let lang = i18n::lang();
        let (title, body) = match (done.finished, done.next) {
            (Phase::Focus, next) => {
                let min = match next {
                    Phase::LongBreak => settings.long_break_min,
                    _ => settings.short_break_min,
                };
                (text(lang, Key::FocusDone), i18n::focus_done_body(lang, min))
            }
            _ => {
                // Lowering the session count while Ready can leave the
                // session number above it; never show "5 dari 4".
                let k = self.pomodoro.session().min(settings.sessions);
                (
                    text(lang, Key::BreakDone),
                    i18n::break_done_body(lang, k, settings.sessions),
                )
            }
        };
        self.queue.retain(|n| n.kind != NoticeKind::Pomodoro);
        if matches!(&self.current, Some((n, _)) if n.kind == NoticeKind::Pomodoro) {
            self.current = None;
        }
        self.queue.push_back(Notice {
            key: POMODORO_KEY.into(),
            source: text(lang, Key::Pomodoro).into(),
            title: title.into(),
            body,
            kind: NoticeKind::Pomodoro,
        });
        effects.push(Effect::PlaySound);
    }

    /// Drops the current notice once it has expired and promotes the next
    /// queued one. A screenshot preview under the pointer does not expire;
    /// one that ran its full time untouched with "Jika diabaikan: Hapus"
    /// asks for the delete (the only way besides the Hapus click).
    fn advance(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        if let Some((n, expires)) = &self.current {
            let held = self.notice_held(n);
            if !held && *expires <= now {
                if let NoticeKind::Screenshot {
                    shot,
                    delete_if_ignored: true,
                    ..
                } = n.kind
                    && !self.shot_touched
                {
                    effects.push(Effect::Shot(ShotAction::DeleteIgnored(shot)));
                }
                self.current = None;
            }
        }
        if self.current.is_none()
            && let Some(next) = self.queue.pop_front()
        {
            let expires = now + duration_of(&next);
            if shot_of(&next).is_some() {
                self.shot_touched = false;
            }
            // It arrives under the pointer, if any: the visit began elsewhere.
            self.visit_on_notice = false;
            self.current = Some((next, expires));
        }
    }

    /// The screenshot preview on screen now, by id.
    fn shown_shot(&self) -> Option<u64> {
        self.current.as_ref().and_then(|(n, _)| shot_of(n))
    }

    /// Screenshot `id`'s preview is on screen or waits in the queue.
    pub fn has_shot(&self, id: u64) -> bool {
        self.shown_shot() == Some(id) || self.queue.iter().any(|n| shot_of(n) == Some(id))
    }

    /// A button of the preview on screen: it closes (the file kept unless
    /// it is Hapus) and `App` does the rest.
    fn shot_button(&mut self, id: u64, action: ShotAction, effects: &mut Vec<Effect>) {
        if self.shown_shot() == Some(id) {
            self.current = None;
            effects.push(Effect::Shot(action));
        }
    }

    /// Removes the sticky notice for `key`, including a brief fullscreen copy
    /// that is queued or showing, so a resolved wait or a dismissed reminder is
    /// never displayed.
    fn clear_sticky(&mut self, key: Sticky) {
        let is_key = |n: &Notice| sticky_key(n) == Some(key);
        self.sticky.retain(|n| !is_key(n));
        self.queue.retain(|n| !is_key(n));
        if matches!(&self.current, Some((n, _)) if is_key(n)) {
            self.current = None;
        }
    }

    /// Dismisses the newest sticky notice: a reminder is then done.
    fn dismiss_newest_sticky(&mut self) -> Option<Effect> {
        let notice = self.sticky.last()?.clone();
        self.dismiss_sticky(&notice)
    }

    /// Removes a sticky notice (and its copies); a reminder is then done.
    fn dismiss_sticky(&mut self, notice: &Notice) -> Option<Effect> {
        self.clear_sticky(sticky_key(notice)?);
        match &notice.kind {
            NoticeKind::Reminder { id } => Some(Effect::ReminderDone(id.clone())),
            NoticeKind::MoleFailed => {
                self.open_mole_log();
                None
            }
            _ => None,
        }
    }

    /// Dismisses whichever notice the view is showing. Only called while the
    /// view is a notice: the current one, else the newest sticky one.
    fn dismiss_visible_notice(&mut self, now: Instant) -> Option<Effect> {
        let Some((notice, _)) = self.current.take() else {
            return self.dismiss_newest_sticky();
        };
        match notice.kind {
            NoticeKind::App {
                app_id: Some(id), ..
            } => Some(Effect::OpenApp(id)),
            NoticeKind::ClaudeWaiting { .. }
            | NoticeKind::Reminder { .. }
            | NoticeKind::MoleFailed => self.dismiss_sticky(&notice),
            NoticeKind::MoleDone => {
                self.open_mole_log();
                None
            }
            NoticeKind::Pomodoro => {
                if self.pomodoro.status() == PomoStatus::Ready {
                    self.pomodoro.start_pause(now);
                }
                None
            }
            // The panel the pointer opens next is the System page.
            NoticeKind::SystemLow => {
                self.page = Page::System;
                None
            }
            // The panel the pointer opens next is the Clipboard's (when that
            // page is on; off, the click only dismisses).
            NoticeKind::ClipPreview { .. } => {
                if self.pages.is_on(Page::Clipboard) {
                    self.page = Page::Clipboard;
                }
                None
            }
            _ => None,
        }
    }
}

/// What the panel draws.
impl IslandState {
    pub fn page(&self) -> Page {
        self.page
    }

    /// "Halaman panel": the order and the pages shown.
    pub fn pages(&self) -> &PanelPages {
        &self.pages
    }

    /// "Pintasan": the tiles the page shows, in order.
    pub fn tiles(&self) -> Vec<Tile> {
        self.tiles.visible()
    }

    /// What the panel shows now, `None` while the view is not the panel.
    pub fn shown_panel(&self) -> Option<ShownPanel> {
        let View::Panel(page) = self.view() else {
            return None;
        };
        Some(ShownPanel {
            page,
            cleaner: self.cleaner,
            mole: self.mole,
            cli_tall: self.cli_tall,
            cli_menu: self.cli_menu,
            notes_page: self.notes_page,
            reminders_page: self.reminders_page,
            day_page: self.day_page,
            strip: self.sticky.last().cloned(),
        })
    }

    /// The sticky notice (Claude waiting or a due reminder) to show at the top
    /// of the open panel.
    pub fn waiting_strip(&self) -> Option<&Notice> {
        match self.view() {
            View::Panel(_) => self.sticky.last(),
            _ => None,
        }
    }

    pub fn pomodoro(&self) -> &Pomodoro {
        &self.pomodoro
    }

    /// The local date as of the last `Event::Today`.
    pub fn today(&self) -> NaiveDate {
        self.today
    }

    pub fn cal_view(&self) -> CalView {
        self.cal_view
    }

    pub fn cal_anchor(&self) -> NaiveDate {
        self.cal_anchor
    }

    pub fn cal_selected(&self) -> NaiveDate {
        self.cal_selected
    }

    /// Dates `[from, to)` the calendar page shows.
    pub fn cal_range(&self) -> (NaiveDate, NaiveDate) {
        range_for(self.cal_view, self.cal_anchor)
    }

    /// How many times "+N lagi" was clicked since the panel opened; the notes
    /// layout wraps it around the list.
    pub fn notes_page(&self) -> usize {
        self.notes_page
    }

    /// How many times the day view's "+N lagi" was clicked on the open day;
    /// the day layout wraps it around that day's events.
    pub fn day_page(&self) -> usize {
        self.day_page
    }

    /// How many times the reminder list's "+N lagi" was clicked since the
    /// panel opened; the layout wraps it around the list.
    pub fn reminders_page(&self) -> usize {
        self.reminders_page
    }

    /// The cleaner over the System page, while it is open.
    pub fn cleaner(&self) -> Option<&CleanerUi> {
        self.cleaner.as_ref()
    }

    /// A clean is running.
    pub fn clean_busy(&self) -> bool {
        self.clean_busy
    }

    /// The cleaner's remembered mode.
    #[cfg(test)]
    pub fn clean_mode(&self) -> CleanMode {
        self.clean_mode
    }

    pub fn mole_phase(&self) -> MolePhase {
        self.mole
    }

    pub fn mole_opts(&self) -> MoleOpts {
        self.mole_opts
    }
}

/// The panel after a shortcut click.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Collapse {
    No,
    /// Shut, the pointer still on this visit.
    Shut,
    /// Shut, and the pointer has left; the next hover opens it again.
    Left,
}

/// What an event does to the linger (`IslandState::linger_trigger`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum LingerTrigger {
    No,
    /// Arms when the panel was open before the event.
    Always,
    /// Arms when the panel was open and something else still holds it
    /// after the event (the pointer: leaving keeps what is left). A panel
    /// the event leaves to close does close, an earlier linger included.
    StillOpen,
    /// Arms when the panel was open and showing its content.
    Panel,
    /// Arms when the panel was open and the page changed.
    PageTurn,
}

impl LingerTrigger {
    fn armed(
        self,
        page_changed: bool,
        panel_was_open: bool,
        panel_view_was: bool,
        open_after: bool,
    ) -> bool {
        match self {
            LingerTrigger::No => false,
            LingerTrigger::Always => panel_was_open,
            LingerTrigger::StillOpen => panel_was_open && open_after,
            LingerTrigger::Panel => panel_was_open && panel_view_was,
            LingerTrigger::PageTurn => panel_was_open && panel_view_was && page_changed,
        }
    }
}

/// Where the visit of a drop stands, in Hover mode (the other modes keep
/// the dropped panel by their own rules, and stay `No`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum DropVisit {
    /// No drop is holding the panel.
    No,
    /// The drop's own visit: the pointer is on the island, or has not been
    /// seen to leave it. Its leaving does not close the panel.
    Here,
    /// That visit is over; the next hover hands the panel to hover.
    Left,
}

/// What a sticky notice is for: one per Claude session, one per reminder.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sticky<'a> {
    Session(&'a str),
    Reminder(&'a str),
    /// The failed Mole clean (one at a time).
    Mole,
}

/// The page the search's back button shows: the one it came over. The search
/// is never its own way back (it should not happen), so `Shortcuts` stands in:
/// `PanelPages::position` has no place for `Page::Search`.
fn back_page(prev: Page) -> Page {
    if prev == Page::Search {
        Page::Shortcuts
    } else {
        prev
    }
}

/// A transport button: previous, play/pause or next.
pub fn is_media_hit(hit: &Hit) -> bool {
    matches!(hit, Hit::Prev | Hit::PlayPause | Hit::Next)
}

fn sticky_key(notice: &Notice) -> Option<Sticky<'_>> {
    match &notice.kind {
        NoticeKind::ClaudeWaiting { session_id } => Some(Sticky::Session(session_id)),
        NoticeKind::Reminder { id } => Some(Sticky::Reminder(id)),
        NoticeKind::MoleFailed => Some(Sticky::Mole),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::mole::MoleOpt;
    use crate::pomodoro::{Phase, PomoSettings, PomoStatus};
    use chrono::NaiveDate;
    use std::time::{Duration, Instant};

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn mins(n: u64) -> Duration {
        Duration::from_secs(60 * n)
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn st() -> IslandState {
        IslandState::new(date(2026, 10, 25), PomoSettings::default())
    }

    /// The pointer leaves and a click lands outside: no linger holds the
    /// panel (for tests of what happens once it is shut).
    fn leave(s: &mut IslandState, now: Instant) -> Vec<Effect> {
        let mut effects = s.apply(Event::Hover(false), now);
        effects.extend(s.apply(Event::Dismiss, now));
        effects
    }

    /// Hovers and scrolls the open panel to `page`.
    fn open_on(s: &mut IslandState, page: Page, now: Instant) {
        s.apply(Event::Hover(true), now);
        let target = Page::ALL.iter().position(|p| *p == page).unwrap() as i32;
        let current = Page::ALL.iter().position(|p| *p == s.page()).unwrap() as i32;
        s.apply(Event::Wheel(-120 * (target - current)), now);
        assert_eq!(s.view(), View::Panel(page));
    }

    fn app_notice(key: &str) -> Notice {
        Notice {
            key: key.into(),
            source: "WhatsApp".into(),
            title: "Ibu".into(),
            body: "udah makan belum?".into(),
            kind: NoticeKind::App {
                app_id: Some("WhatsApp.Desktop!App".into()),
                toast: None,
            },
        }
    }

    fn media(playing: bool) -> MediaInfo {
        MediaInfo {
            title: "Bohemian Rhapsody".into(),
            artist: "Queen".into(),
            playing,
            art: None,
            source_app: "Chrome".into(),
        }
    }

    fn waiting(session: &str) -> Notice {
        Notice {
            key: format!("claude-waiting-{session}"),
            source: "Claude Code".into(),
            title: "Claude is waiting".into(),
            body: "Needs your input".into(),
            kind: NoticeKind::ClaudeWaiting {
                session_id: session.into(),
            },
        }
    }

    fn done(session: &str) -> Notice {
        Notice {
            key: format!("claude-done-{session}"),
            source: "Claude Code".into(),
            title: "Claude is done".into(),
            body: "Task finished".into(),
            kind: NoticeKind::ClaudeDone {
                session_id: session.into(),
            },
        }
    }

    fn notice_title(view: &View) -> Option<(&NoticeKind, &str, &str)> {
        match view {
            View::Notice(n) => Some((&n.kind, n.title.as_str(), n.body.as_str())),
            _ => None,
        }
    }

    fn greeting() -> Notice {
        Notice {
            key: "greeting".into(),
            source: "Bata".into(),
            title: "Selamat pagi".into(),
            body: "3 acara dan 2 pengingat hari ini".into(),
            kind: NoticeKind::Greeting {
                period: crate::greeting::DayPeriod::Morning,
            },
        }
    }

    #[test]
    fn greeting_shows_four_seconds_and_never_in_fullscreen() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(greeting()), t0);
        assert_eq!(s.view(), View::Notice(greeting()));
        assert_eq!(s.next_deadline(), Some(t0 + secs(4)));
        s.apply(Event::Tick, t0 + secs(4));
        assert_eq!(s.view(), View::Idle);
        // Fullscreen: dropped, not queued for afterwards.
        s.apply(Event::Fullscreen(true), t0);
        assert!(s.fullscreen());
        s.apply(Event::Notice(greeting()), t0);
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Fullscreen(false), t0);
        assert_eq!(s.view(), View::Idle);
        // A click dismisses it like any notice.
        s.apply(Event::Notice(greeting()), t0);
        let effects = s.apply(Event::Click(Hit::Body), t0);
        assert!(effects.is_empty());
        assert_eq!(s.view(), View::Idle);
    }

    // ---- v1 behaviour, adapted to v2 views ----

    #[test]
    fn media_shows_compact_and_hover_opens_panel() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        assert_eq!(s.view(), View::MediaCompact);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Wheel(-480), t0);
        assert_eq!(s.view(), View::Panel(Page::Media));
        leave(&mut s, t0);
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn paused_media_still_shows() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(false))), t0);
        assert_eq!(s.view(), View::MediaCompact);
        assert_eq!(s.media(), Some(&media(false)));
    }

    #[test]
    fn media_gone_returns_to_idle() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        assert_eq!(s.view(), View::MediaCompact);
        s.apply(Event::Media(None), t0);
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.media(), None);
    }

    #[test]
    fn notice_shows_then_returns_to_media() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        s.apply(Event::Tick, t0 + Duration::from_millis(4900));
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn burst_of_notices_shows_each_in_order() {
        let t0 = Instant::now();
        let mut s = st();
        for i in 0..30 {
            s.apply(Event::Notice(app_notice(&format!("k{i}"))), t0);
        }
        for i in 0..30u32 {
            let at = t0 + NOTICE_DURATION * i;
            assert_eq!(
                s.view(),
                View::Notice(app_notice(&format!("k{i}"))),
                "at notice {i}"
            );
            assert_eq!(s.next_deadline(), Some(at + NOTICE_DURATION));
            s.apply(Event::Tick, t0 + NOTICE_DURATION * (i + 1));
        }
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn fullscreen_hides_media_but_not_new_notice() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        s.apply(Event::Fullscreen(true), t0);
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Fullscreen(false), t0 + secs(5));
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn claude_waiting_is_sticky_until_same_session_activity() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        assert_eq!(s.view(), View::Notice(waiting("s1")));
        s.apply(Event::Tick, t0 + secs(60));
        assert_eq!(s.view(), View::Notice(waiting("s1")));
        s.apply(
            Event::ClaudeActivity {
                session_id: "s2".into(),
            },
            t0 + secs(61),
        );
        assert_eq!(s.view(), View::Notice(waiting("s1")));
        s.apply(
            Event::ClaudeActivity {
                session_id: "s1".into(),
            },
            t0 + secs(62),
        );
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn claude_done_clears_waiting_of_same_session() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(done("s1")), t0);
        assert_eq!(s.view(), View::Notice(done("s1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn temporary_notice_preempts_waiting_then_waiting_returns() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Notice(waiting("s1")));
    }

    #[test]
    fn click_app_notice_opens_app_and_dismisses() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(app_notice("k1")), t0);
        let effects = s.apply(Event::Click(Hit::Body), t0 + secs(1));
        assert_eq!(
            effects,
            vec![Effect::OpenApp("WhatsApp.Desktop!App".into())]
        );
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn click_media_buttons_emit_commands() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        open_on(&mut s, Page::Media, t0);
        // Once the hover guard has passed.
        let t = t0 + MEDIA_HOVER_GUARD;
        assert_eq!(
            s.apply(Event::Click(Hit::Next), t),
            vec![Effect::Media(MediaCommand::Next)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::Prev), t),
            vec![Effect::Media(MediaCommand::Prev)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::PlayPause), t),
            vec![Effect::Media(MediaCommand::Pause)]
        );
        assert_eq!(s.view(), View::Panel(Page::Media));
    }

    #[test]
    fn play_pause_follows_the_shown_state() {
        let t0 = Instant::now();
        let t = t0 + MEDIA_HOVER_GUARD;
        let mut s = st();
        open_on(&mut s, Page::Media, t0);
        // No media: nothing to command.
        assert_eq!(s.apply(Event::Click(Hit::PlayPause), t), vec![]);
        s.apply(Event::Media(Some(media(true))), t);
        assert_eq!(
            s.apply(Event::Click(Hit::PlayPause), t),
            vec![Effect::Media(MediaCommand::Pause)],
            "playing pauses"
        );
        s.apply(Event::Media(Some(media(false))), t);
        assert_eq!(
            s.apply(Event::Click(Hit::PlayPause), t),
            vec![Effect::Media(MediaCommand::Play)],
            "paused plays"
        );
    }

    #[test]
    fn media_buttons_wait_after_a_hover_open() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        open_on(&mut s, Page::Media, t0);
        leave(&mut s, t0);
        // Hover reopens it on the Music page, under a passing pointer.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Media));
        let early = t0 + Duration::from_millis(100);
        assert!(s.media_guarded(early));
        assert_eq!(s.hover_open_age(early), Some(Duration::from_millis(100)));
        for hit in [Hit::PlayPause, Hit::Prev, Hit::Next] {
            assert_eq!(s.apply(Event::Click(hit.clone()), early), vec![], "{hit:?}");
        }
        // Just before the window ends: still ignored.
        let edge = t0 + MEDIA_HOVER_GUARD - Duration::from_millis(1);
        assert_eq!(s.apply(Event::Click(Hit::PlayPause), edge), vec![]);
        let after = t0 + MEDIA_HOVER_GUARD;
        assert!(!s.media_guarded(after));
        assert_eq!(
            s.apply(Event::Click(Hit::PlayPause), after),
            vec![Effect::Media(MediaCommand::Pause)]
        );
        // Closing clears it; a later hover open starts a new window.
        leave(&mut s, after);
        assert_eq!(s.hover_open_age(after), None);
        let later = t0 + secs(10);
        s.apply(Event::Hover(true), later);
        assert_eq!(s.apply(Event::Click(Hit::PlayPause), later), vec![]);
    }

    #[test]
    fn the_hover_guard_spares_other_buttons() {
        let t0 = Instant::now();
        let early = t0 + Duration::from_millis(100);
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        open_on(&mut s, Page::Media, t0);
        assert!(s.media_guarded(early));
        assert_eq!(
            s.apply(Event::Click(Hit::LyricsToggle), early),
            vec![Effect::SaveLyrics(true)]
        );
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Click(Hit::CalNext), early);
        assert_ne!(s.cal_anchor(), date(2026, 10, 25), "calendar moved");
    }

    #[test]
    fn a_panel_held_open_without_hover_is_not_guarded() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(false))), t0);
        // An editor opens the panel: no hover open, no guard.
        s.apply(Event::EditorOpen(true), t0);
        assert!(matches!(s.view(), View::Panel(_)));
        assert!(!s.media_guarded(t0));
        assert_eq!(
            s.apply(Event::Click(Hit::PlayPause), t0),
            vec![Effect::Media(MediaCommand::Play)]
        );
        // Hover arriving while the editor already holds it open: still none.
        s.apply(Event::Hover(true), t0);
        assert!(!s.media_guarded(t0));
        assert_eq!(
            s.apply(Event::Click(Hit::Next), t0),
            vec![Effect::Media(MediaCommand::Next)]
        );
        // Same for the CLI input's focus: it holds the panel once the pointer
        // has gone, and a return of the pointer opens nothing new.
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::CliFocus(true), t0);
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Panel(Page::Cli));
        assert!(!s.media_guarded(t0), "cleared once hover went");
        s.apply(Event::Hover(true), t0);
        assert!(!s.media_guarded(t0));
        assert_eq!(
            s.apply(Event::Click(Hit::Prev), t0),
            vec![Effect::Media(MediaCommand::Prev)]
        );
    }

    #[test]
    fn click_body_on_media_does_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![]);
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn click_waiting_notice_dismisses_it() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        s.apply(Event::Notice(waiting("s1")), t0);
        assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![]);
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn waiting_is_tracked_per_session() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(waiting("s2")), t0);
        assert_eq!(s.view(), View::Notice(waiting("s2")));
        s.apply(
            Event::ClaudeActivity {
                session_id: "s2".into(),
            },
            t0,
        );
        assert_eq!(s.view(), View::Notice(waiting("s1")));
    }

    #[test]
    fn repeated_waiting_moves_session_to_newest() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(waiting("s2")), t0);
        s.apply(Event::Notice(waiting("s1")), t0);
        assert_eq!(s.view(), View::Notice(waiting("s1")));
        s.apply(
            Event::ClaudeActivity {
                session_id: "s1".into(),
            },
            t0,
        );
        assert_eq!(s.view(), View::Notice(waiting("s2")));
    }

    #[test]
    fn click_dismisses_only_the_visible_waiting_notice() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(waiting("s2")), t0);
        assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![]);
        assert_eq!(s.view(), View::Notice(waiting("s1")));
    }

    #[test]
    fn waiting_during_fullscreen_shows_briefly_and_returns_after() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(waiting("s1")), t0);
        assert_eq!(s.view(), View::Notice(waiting("s1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Fullscreen(false), t0 + secs(6));
        assert_eq!(s.view(), View::Notice(waiting("s1")));
    }

    #[test]
    fn activity_during_fullscreen_clears_brief_waiting_notice() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(
            Event::ClaudeActivity {
                session_id: "s1".into(),
            },
            t0 + secs(1),
        );
        assert_eq!(s.view(), View::Hidden);
        assert_eq!(s.next_deadline(), None);
        s.apply(Event::Fullscreen(false), t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn next_deadline_is_current_notice_expiry() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(s.next_deadline(), None);
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(s.next_deadline(), Some(t0 + secs(5)));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.next_deadline(), None);
    }

    /// A notice with nothing to press (the battery warning).
    fn plain_notice(key: &str) -> Notice {
        Notice {
            key: key.into(),
            source: "Vale".into(),
            title: "Baterai".into(),
            body: "20%".into(),
            kind: NoticeKind::BatteryLow,
        }
    }

    #[test]
    fn hover_during_notice_is_invisible_but_opens_media_page_after() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        open_on(&mut s, Page::Media, t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Notice(plain_notice("k1")), t0);
        s.apply(Event::Hover(true), t0 + secs(1));
        assert_eq!(s.view(), View::Notice(plain_notice("k1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Panel(Page::Media));
    }

    #[test]
    fn hover_on_a_notice_with_a_button_does_not_open_the_panel() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(app_notice("k1")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        assert!(!s.panel_is_open());
        // The pointer sits on it: no expiry, however long.
        assert_eq!(s.next_deadline(), None);
        s.apply(Event::Tick, t0 + secs(60));
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        // It leaves: a grace first, then the notice goes, and the panel did
        // not pop open under the pointer meanwhile.
        s.apply(Event::Hover(false), t0 + secs(60));
        s.apply(Event::Tick, t0 + secs(61));
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        s.apply(Event::Tick, t0 + secs(63));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn a_click_on_a_notice_does_not_pop_the_panel_under_the_pointer() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(app_notice("k1")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::Body), t0),
            vec![Effect::OpenApp("WhatsApp.Desktop!App".into())]
        );
        assert_eq!(s.view(), View::Idle, "no panel under the pointer");
        // A new visit opens it as usual.
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn notices_that_lead_to_the_panel_open_it_on_the_click() {
        let t0 = Instant::now();
        let mut s = st();
        let low = Notice {
            key: "sys".into(),
            source: "Vale".into(),
            title: "Disk".into(),
            body: "9 GB".into(),
            kind: NoticeKind::SystemLow,
        };
        s.apply(Event::Notice(low), t0);
        s.apply(Event::Hover(true), t0);
        assert!(!s.panel_is_open());
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Panel(Page::System));
    }

    #[test]
    fn a_reminder_keeps_hover_off_until_a_wheel_notch_or_a_new_visit() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(reminder("r1")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        // Selesai: the reminder is done, and the panel stays shut.
        assert_eq!(
            s.apply(Event::Click(Hit::Body), t0),
            vec![Effect::ReminderDone("r1".into())]
        );
        assert_eq!(s.view(), View::Idle);
        // Another reminder: a wheel notch peeks at the panel (its strip),
        // and that notch only opens it: the page stays.
        s.apply(Event::Notice(reminder("r2")), t0);
        assert_eq!(s.view(), View::Notice(reminder("r2")));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert_eq!(s.waiting_strip(), Some(&reminder("r2")));
        // The next notch turns the page as usual.
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
        // Leaving and coming back is a new visit: the first one since r2
        // appeared keeps hover off.
        leave(&mut s, t0);
        assert_eq!(s.view(), View::Notice(reminder("r2")));
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(reminder("r2")));
        // Only that one: the visit after it opens the panel with the strip.
        leave(&mut s, t0);
        s.apply(Event::Hover(true), t0);
        assert!(matches!(s.view(), View::Panel(_)));
        assert_eq!(s.waiting_strip(), Some(&reminder("r2")));
    }

    #[test]
    fn a_waiting_reminder_opens_the_panel_with_its_strip_after_one_visit() {
        // The v3.1 order: the reminder is there before the pointer is.
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        // The first visit is for its Selesai button.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        s.apply(Event::Hover(false), t0);
        // Every later visit opens the panel, the reminder in the strip.
        for _ in 0..3 {
            s.apply(Event::Hover(true), t0);
            assert_eq!(s.view(), View::Panel(Page::Calendar));
            assert_eq!(s.waiting_strip(), Some(&reminder("r1")));
            s.apply(Event::Hover(false), t0);
        }
        // A new reminder gets its own first visit.
        s.apply(Event::Notice(reminder("r2")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(reminder("r2")));
    }

    #[test]
    fn the_wheel_over_a_temporary_notice_leaves_the_panel_shut() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::HoverStay, t0);
        s.apply(Event::Notice(app_notice("k1")), t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        assert!(!s.outside_click_wanted(), "nothing latched behind it");
        // It goes once the pointer leaves: nothing was kept open.
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Tick, t0 + secs(60));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.page(), Page::Calendar, "the notch turned no page");
    }

    #[test]
    fn a_notice_with_a_button_over_an_open_panel_runs_its_time_in_every_mode() {
        let t0 = Instant::now();
        for m in [OpenMode::Hover, OpenMode::HoverStay, OpenMode::Click] {
            let mut s = mode(m, t0);
            s.apply(Event::Hover(true), t0);
            if m == OpenMode::Click {
                s.apply(Event::Click(Hit::Body), t0);
            }
            assert_eq!(s.view(), View::Panel(Page::Calendar), "{m:?}");
            // It arrives under the pointer, over the open panel.
            s.apply(Event::Notice(app_notice("k1")), t0 + secs(1));
            assert_eq!(s.view(), View::Notice(app_notice("k1")), "{m:?}");
            assert_eq!(s.next_deadline(), Some(t0 + secs(6)), "{m:?}");
            s.apply(Event::Tick, t0 + secs(6));
            assert_eq!(
                s.view(),
                View::Panel(Page::Calendar),
                "{m:?}: the panel is back"
            );
        }
        // A visit that begins on it still holds it.
        let mut s = st();
        s.apply(Event::Notice(app_notice("k1")), t0);
        s.apply(Event::Hover(true), t0 + secs(1));
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn hover_and_stay_latches_only_once_the_panel_shows() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::HoverStay, t0);
        // A plain notice covers the panel: the pointer passing does not keep it.
        s.apply(Event::Notice(plain_notice("k1")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(plain_notice("k1")));
        s.apply(Event::Hover(false), t0 + secs(1));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Idle, "not opened behind the notice");
        // The notice ends under the pointer: the panel shows, and stays.
        s.apply(Event::Notice(plain_notice("k2")), t0 + secs(10));
        s.apply(Event::Hover(true), t0 + secs(10));
        s.apply(Event::Tick, t0 + secs(15));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Hover(false), t0 + secs(16));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert!(s.outside_click_wanted());
    }

    #[test]
    fn notices_that_lead_to_the_panel_keep_it_open_in_the_staying_modes() {
        let t0 = Instant::now();
        for m in [OpenMode::HoverStay, OpenMode::Click] {
            let mut s = mode(m, t0);
            s.apply(
                Event::Notice(Notice {
                    key: "sys".into(),
                    source: "Vale".into(),
                    title: "Disk".into(),
                    body: "9 GB".into(),
                    kind: NoticeKind::SystemLow,
                }),
                t0,
            );
            s.apply(Event::Hover(true), t0);
            s.apply(Event::Click(Hit::Body), t0);
            assert_eq!(s.view(), View::Panel(Page::System), "{m:?}");
            s.apply(Event::Hover(false), t0 + secs(1));
            assert_eq!(s.view(), View::Panel(Page::System), "{m:?}: it stays");
            assert!(s.outside_click_wanted(), "{m:?}");
        }
    }

    #[test]
    fn a_click_before_the_hover_dwell_still_guards_the_media_buttons() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Click, t0);
        s.apply(Event::Media(Some(media(true))), t0);
        // The click lands before `Hover(true)` (the dwell) arrives.
        s.apply(Event::Click(Hit::Body), t0);
        assert!(matches!(s.view(), View::Panel(_)));
        let early = t0 + Duration::from_millis(100);
        assert!(s.media_guarded(early));
        for hit in [Hit::Prev, Hit::PlayPause, Hit::Next] {
            assert_eq!(s.apply(Event::Click(hit.clone()), early), vec![], "{hit:?}");
        }
        // The dwell arrives during the morph: still guarded.
        s.apply(Event::Hover(true), t0 + Duration::from_millis(150));
        assert!(s.media_guarded(t0 + Duration::from_millis(200)));
        assert_eq!(
            s.apply(Event::Click(Hit::PlayPause), t0 + MEDIA_HOVER_GUARD),
            vec![Effect::Media(MediaCommand::Pause)]
        );
    }

    #[test]
    fn a_reminder_over_an_open_panel_is_a_strip_and_the_panel_stays() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert_eq!(s.waiting_strip(), Some(&reminder("r1")));
    }

    // ---- v3.2: open mode ----

    fn mode(m: OpenMode, t: Instant) -> IslandState {
        let mut s = st();
        s.apply(Event::OpenMode(m), t);
        s
    }

    #[test]
    fn hover_and_stay_keeps_the_panel_until_a_click_outside() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::HoverStay, t0);
        assert!(!s.outside_click_wanted());
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert!(s.outside_click_wanted());
        // The pointer leaves: it stays.
        s.apply(Event::Hover(false), t0 + secs(1));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert!(s.outside_click_wanted());
        // A click outside closes it, and the watching stops.
        s.apply(Event::Dismiss, t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
        assert!(!s.outside_click_wanted());
        // The pointer comes back: it opens again.
        s.apply(Event::Hover(true), t0 + secs(3));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn hover_and_stay_dismissed_under_the_pointer_waits_for_a_new_visit() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::HoverStay, t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Dismiss, t0);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn what_else_holds_the_panel_survives_a_click_outside() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::HoverStay, t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::EditorOpen(true), t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Dismiss, t0);
        assert!(s.panel_is_open(), "an open editor keeps the panel");
        s.apply(Event::EditorOpen(false), t0);
        // The editor's close lingers a moment, then the panel goes.
        assert!(s.panel_is_open());
        s.apply(Event::Tick, t0 + ACTIVITY_LINGER);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn a_shortcut_or_a_game_lets_a_kept_panel_go() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::HoverStay, t0);
        open_on(&mut s, Page::Clipboard, t0);
        s.apply(Event::Hover(false), t0);
        assert!(s.panel_is_open());
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Fullscreen(false), t0 + secs(1));
        assert_eq!(s.view(), View::Idle, "a fullscreen app closed it");
        // And the panel getting out of the way (a copy with Enter) does too.
        s.apply(Event::Hover(true), t0 + secs(2));
        assert!(s.panel_is_open());
        s.apply(Event::ClipCommit, t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(false), t0 + secs(3));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn click_mode_opens_on_a_click_and_hover_does_not() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Click, t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Idle, "hover alone opens nothing");
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert!(s.outside_click_wanted());
        // The media buttons wait out the opening, as after a hover.
        assert!(s.media_guarded(t0 + Duration::from_millis(100)));
        // Leaving keeps it; a click outside closes it.
        s.apply(Event::Hover(false), t0 + secs(1));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Dismiss, t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn click_mode_opens_over_music_and_a_ready_pomodoro_without_starting_it() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Click, t0);
        s.apply(Event::Media(Some(media(true))), t0);
        assert_eq!(s.view(), View::MediaCompact);
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Dismiss, t0);
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn click_mode_still_dismisses_a_notice_with_a_click() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Click, t0);
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::Body), t0),
            vec![Effect::OpenApp("WhatsApp.Desktop!App".into())]
        );
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn click_mode_opens_the_panel_over_a_sticky_notice_and_its_strip_keeps_it() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Click, t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        // The click opens the panel; the reminder is not marked done.
        let effects = s.apply(Event::Click(Hit::Body), t0);
        assert!(
            !effects.contains(&Effect::ReminderDone("r1".into())),
            "{effects:?}"
        );
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert_eq!(s.waiting_strip(), Some(&reminder("r1")));
        // The strip's own click is Selesai.
        assert_eq!(
            s.apply(Event::Click(Hit::Strip), t0),
            vec![Effect::ReminderDone("r1".into())]
        );
        // Claude waiting likewise.
        let mut s = mode(OpenMode::Click, t0);
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.waiting_strip(), Some(&waiting("s1")));
    }

    #[test]
    fn changing_the_mode_lets_a_kept_panel_go() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::HoverStay, t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Hover(false), t0);
        assert!(s.panel_is_open());
        s.apply(Event::OpenMode(OpenMode::Hover), t0);
        assert_eq!(s.view(), View::Idle);
        assert!(!s.outside_click_wanted());
    }

    // ---- v2: idle, panel, pages, wheel ----

    #[test]
    fn idle_shows_empty_shape() {
        assert_eq!(st().view(), View::Idle);
    }

    #[test]
    fn fullscreen_hides_idle() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        assert_eq!(s.view(), View::Hidden);
    }

    #[test]
    fn fullscreen_hides_open_panel() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Fullscreen(true), t0);
        assert_eq!(s.view(), View::Hidden);
    }

    #[test]
    fn hover_opens_panel_on_last_page() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
        leave(&mut s, t0);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
        assert_eq!(s.page(), Page::Pomodoro);
    }

    #[test]
    fn small_wheel_deltas_accumulate_to_one_page() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        for i in 0..3 {
            s.apply(Event::Wheel(-30), t0);
            assert_eq!(s.view(), View::Panel(Page::Calendar), "after {i}");
        }
        s.apply(Event::Wheel(-30), t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
        s.apply(Event::Wheel(-119), t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
        s.apply(Event::Wheel(-1), t0);
        assert_eq!(s.view(), View::Panel(Page::Reminders));
    }

    #[test]
    fn wheel_up_goes_to_previous_page() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        s.apply(Event::Wheel(120), t0);
        assert_eq!(s.view(), View::Panel(Page::Reminders));
    }

    #[test]
    fn wheel_clamps_at_ends() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Wheel(120), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Wheel(-120 * 20), t0);
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
        // Scrolling past the end is not stored up: one notch back moves one page.
        s.apply(Event::Wheel(120), t0);
        assert_eq!(s.view(), View::Panel(Page::Arrange));
    }

    #[test]
    fn a_sideways_flick_turns_one_page_each_way() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.page(), Page::Calendar);
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.view(), View::Panel(Page::Reminders));
        s.apply(Event::PageFlick { next: false }, t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
    }

    #[test]
    fn a_sideways_flick_skips_hidden_pages_like_the_wheel() {
        let t0 = Instant::now();
        let mut s = st();
        let mut pages = PanelPages::default();
        pages.toggle(Page::Pomodoro);
        pages.toggle(Page::Reminders);
        s.apply(Event::PanelPages(pages), t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.view(), View::Panel(Page::Notes));
        s.apply(Event::PageFlick { next: false }, t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn a_sideways_flick_clamps_at_the_ends_and_drops_wheel_residue() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::PageFlick { next: false }, t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        // A vertical residue of 90 must not add a second step to the flick.
        s.apply(Event::Wheel(-90), t0);
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
        s.apply(Event::Wheel(-60), t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro), "the residue is gone");
        for _ in 0..20 {
            s.apply(Event::PageFlick { next: true }, t0);
        }
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
        s.apply(Event::PageFlick { next: false }, t0);
        assert_eq!(s.view(), View::Panel(Page::Arrange));
    }

    #[test]
    fn a_sideways_flick_never_turns_the_search_or_a_closed_panel() {
        let t0 = Instant::now();
        let mut s = st();
        // Idle: nothing.
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.page(), Page::Calendar);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::SearchHotkey, t0);
        assert_eq!(s.view(), View::Panel(Page::Search));
        for next in [true, false] {
            assert!(s.apply(Event::PageFlick { next }, t0).is_empty());
            assert_eq!(s.view(), View::Panel(Page::Search));
        }
        s.apply(Event::SearchClose, t0);
        assert_eq!(s.page(), Page::Calendar);
    }

    #[test]
    fn a_sideways_flick_lingers_like_a_page_turn() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.page(), Page::Pomodoro);
        assert_eq!(s.next_deadline(), Some(t0 + secs(4)));
        s.apply(Event::Hover(false), t0 + secs(1));
        assert!(s.panel_is_open());
        // A flick that turned no page arms nothing.
        let mut e = st();
        e.apply(Event::Hover(true), t0);
        e.apply(Event::PageFlick { next: false }, t0);
        assert_eq!(e.next_deadline(), None);
    }

    #[test]
    fn a_flick_spent_on_the_sticky_reminder_peek_turns_no_page() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(reminder("r1")), t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Click(Hit::Body), t0);
        s.apply(Event::Notice(reminder("r2")), t0);
        // The wheel's rule: the first one only opens the panel.
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::PageFlick { next: true }, t0);
        assert_eq!(s.view(), View::Panel(Page::Pomodoro));
    }

    #[test]
    fn a_dock_click_turns_to_that_page_and_lingers_like_a_page_turn() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        // A vertical residue must not add a step to the click.
        s.apply(Event::Wheel(-90), t0);
        s.apply(Event::Click(Hit::PanelPage(Page::Media)), t0);
        assert_eq!(s.view(), View::Panel(Page::Media));
        assert_eq!(
            s.next_deadline(),
            Some(t0 + secs(4)),
            "armed as a page turn"
        );
        s.apply(Event::Wheel(-60), t0);
        assert_eq!(s.view(), View::Panel(Page::Media), "the residue is gone");
        s.apply(Event::Hover(false), t0 + secs(1));
        assert!(s.panel_is_open(), "the linger holds the panel");
        // Back to an earlier page is as good.
        s.apply(Event::Click(Hit::PanelPage(Page::Calendar)), t0 + secs(2));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn a_dock_click_on_the_current_page_does_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Click(Hit::PanelPage(Page::Calendar)), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert_eq!(s.next_deadline(), None, "no page turned, no linger");
    }

    #[test]
    fn a_dock_click_never_acts_on_a_closed_panel_or_the_search() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PanelPage(Page::Media)), t0);
        assert_eq!(s.page(), Page::Calendar);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::SearchHotkey, t0);
        s.apply(Event::Click(Hit::PanelPage(Page::Media)), t0);
        assert_eq!(s.view(), View::Panel(Page::Search));
    }

    #[test]
    fn a_dock_click_spent_on_the_sticky_reminder_peek_turns_no_page() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(reminder("r1")), t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Click(Hit::Body), t0);
        s.apply(Event::Notice(reminder("r2")), t0);
        s.apply(Event::Click(Hit::PanelPage(Page::Media)), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Click(Hit::PanelPage(Page::Media)), t0);
        assert_eq!(s.view(), View::Panel(Page::Media));
    }

    #[test]
    fn wheel_skips_pages_turned_off_in_the_settings() {
        let t0 = Instant::now();
        let mut s = st();
        let mut pages = PanelPages::default();
        pages.toggle(Page::Pomodoro);
        pages.toggle(Page::Reminders);
        s.apply(Event::PanelPages(pages), t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Notes));
        s.apply(Event::Wheel(120), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn the_page_shown_moves_off_a_page_just_turned_off() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        let mut pages = PanelPages::default();
        pages.toggle(Page::Notes);
        s.apply(Event::PanelPages(pages), t0);
        // The next shown page after it; the wheel then walks the rest.
        assert_eq!(s.page(), Page::Media);
        assert_eq!(s.pages().visible().len(), Page::ALL.len() - 1);
    }

    #[test]
    fn a_page_turned_off_does_not_come_back_on_the_next_hover() {
        let t0 = Instant::now();
        let mut s = st();
        let mut pages = PanelPages::default();
        pages.toggle(Page::Notes);
        s.apply(Event::PanelPages(pages), t0);
        // Opened once by its own button (here: set while hovering).
        s.apply(Event::Hover(true), t0);
        s.page = Page::Notes;
        assert_eq!(s.view(), View::Panel(Page::Notes));
        s.apply(Event::Hover(false), t0);
        // The next visit starts on a page that is on.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Media));
    }

    #[test]
    fn pages_end_with_system_devices_cli_then_shortcuts() {
        assert_eq!(
            Page::ALL,
            [
                Page::Calendar,
                Page::Pomodoro,
                Page::Reminders,
                Page::Notes,
                Page::Media,
                Page::Mixer,
                Page::Clipboard,
                Page::Notifications,
                Page::Shelf,
                Page::System,
                Page::Devices,
                Page::Cli,
                Page::Arrange,
                Page::Shortcuts,
            ]
        );
    }

    #[test]
    fn the_mixer_takes_levels_and_mutes_only_while_its_page_is_open() {
        let t0 = Instant::now();
        let mut s = st();
        // Closed, or another page: nothing.
        assert!(s.apply(Event::MixerLevel(1, 0.4), t0).is_empty());
        assert!(s.apply(Event::Click(Hit::MixerMute(1)), t0).is_empty());
        open_on(&mut s, Page::Notes, t0);
        assert!(s.apply(Event::MixerLevel(1, 0.4), t0).is_empty());
        assert!(s.apply(Event::Click(Hit::MixerMute(0)), t0).is_empty());
        open_on(&mut s, Page::Mixer, t0);
        assert_eq!(
            s.apply(Event::MixerLevel(1, 0.4), t0),
            vec![Effect::MixerLevel(1, 0.4)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::MixerMute(2)), t0),
            vec![Effect::MixerMute(2)]
        );
        // The slider's own click (a press already set it) does nothing, and
        // a reading is the app's.
        assert!(s.apply(Event::Click(Hit::MixerSlider(1)), t0).is_empty());
        assert!(s.apply(Event::Audio(1, None), t0).is_empty());
        assert_eq!(s.view(), View::Panel(Page::Mixer));
    }

    fn system_notice() -> Notice {
        Notice {
            key: "system-disk-C".into(),
            source: "Sistem".into(),
            title: "Disk C: tinggal 9,6 GB".into(),
            body: "Buka halaman Sistem untuk detailnya".into(),
            kind: NoticeKind::SystemLow,
        }
    }

    #[test]
    fn system_notice_queues_and_its_click_opens_the_system_page() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(system_notice()), t0);
        assert_eq!(s.view(), View::Notice(system_notice()));
        // Temporary like the battery's.
        assert_eq!(s.next_deadline(), Some(t0 + NOTICE_DURATION));
        // The pointer is on it: the click dismisses it and the panel opens
        // on Sistem, not on the page it was left on.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(system_notice()));
        let effects = s.apply(Event::Click(Hit::Body), t0);
        assert!(effects.is_empty());
        assert_eq!(s.view(), View::Panel(Page::System));
        // Without hover the next visit opens there.
        let mut s = st();
        s.apply(Event::Notice(system_notice()), t0);
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::System));
        // A sample means nothing to the reducer.
        let before = s.view();
        let sample = crate::system::SystemSample {
            kind: crate::system::SampleKind::Warn,
            cpu: 0.0,
            cores: 0,
            ram_used: 1,
            ram_total: 2,
            drives: vec![],
            procs: vec![],
            uptime: Duration::ZERO,
        };
        assert!(s.apply(Event::SystemSample(sample), t0).is_empty());
        assert_eq!(s.view(), before);
    }

    #[test]
    fn system_page_clicks() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::System, t0);
        // A process row runs Task Manager like its tile: the panel shuts.
        let effects = s.apply(Event::Click(Hit::SystemProcess), t0);
        assert_eq!(effects, vec![Effect::RunShortcut(Shortcut::TaskManager)]);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::System));
        // A drive opens Explorer there, the same way.
        let effects = s.apply(Event::Click(Hit::SystemDrive('D')), t0);
        assert_eq!(effects, vec![Effect::OpenDrive('D')]);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Hover(true), t0);
        // "Bersihkan" opens the cleaner over the page.
        assert_eq!(
            s.apply(Event::Click(Hit::SystemClean), t0),
            vec![Effect::OpenCleaner]
        );
        assert_eq!(s.view(), View::Panel(Page::System));
        assert!(s.cleaner().is_some());
        // Off the page, or with the panel shut, nothing.
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Devices));
        assert!(s.apply(Event::Click(Hit::SystemClean), t0).is_empty());
        leave(&mut s, t0);
        assert!(s.apply(Event::Click(Hit::SystemProcess), t0).is_empty());
        assert!(s.apply(Event::Click(Hit::SystemDrive('C')), t0).is_empty());
    }

    /// The System page with the cleaner open over it.
    fn cleaner_open(now: Instant) -> IslandState {
        let mut s = st();
        open_on(&mut s, Page::System, now);
        assert_eq!(
            s.apply(Event::Click(Hit::SystemClean), now),
            vec![Effect::OpenCleaner]
        );
        s
    }

    fn default_picks() -> Vec<crate::clean::CleanCat> {
        use crate::clean::CleanCat;
        vec![
            CleanCat::TempLogs,
            CleanCat::BrowserCache,
            CleanCat::AppCache,
        ]
    }

    #[test]
    fn the_shown_panel_keeps_what_closing_it_resets() {
        use crate::ui::geometry::{ShapeKind, panel_kind};
        let t0 = Instant::now();
        // The cleaner: gone from the state once the panel closes, still in
        // what it showed, so the closing panel keeps the cleaner's size.
        let mut s = cleaner_open(t0);
        let shown = s.shown_panel().expect("the panel shows");
        assert_eq!(shown.page, Page::System);
        assert!(shown.cleaner.is_some());
        leave(&mut s, t0);
        assert!(s.shown_panel().is_none());
        assert!(s.cleaner().is_none());
        assert_eq!(
            panel_kind(shown.page, shown.cli_tall, shown.cleaner.is_some()),
            ShapeKind::PanelClean
        );
        // The tall CLI: back to small in the state, tall in what it showed.
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::Click(Hit::CliExpand), t0);
        let shown = s.shown_panel().expect("the panel shows");
        leave(&mut s, t0);
        assert!(!s.cli_tall() && shown.cli_tall);
        assert_eq!(
            panel_kind(shown.page, shown.cli_tall, shown.cleaner.is_some()),
            ShapeKind::PanelTall
        );
    }

    #[test]
    fn cleaner_opens_over_the_system_page_and_goes_with_it() {
        use crate::clean::CleanCat;
        let t0 = Instant::now();
        let mut s = cleaner_open(t0);
        // A sub-view: the page is still Sistem.
        assert_eq!(s.view(), View::Panel(Page::System));
        let c = *s.cleaner().expect("open");
        assert_eq!(c.picked(), default_picks());
        assert!(!c.armed() && !s.clean_busy());
        // A second "Bersihkan" (no region over the cleaner) changes nothing.
        assert!(s.apply(Event::Click(Hit::SystemClean), t0).is_empty());
        // Ticks toggle.
        s.apply(Event::Click(Hit::CleanToggle(CleanCat::DevCache)), t0);
        s.apply(Event::Click(Hit::CleanToggle(CleanCat::TempLogs)), t0);
        assert_eq!(
            s.cleaner().unwrap().picked(),
            [
                CleanCat::BrowserCache,
                CleanCat::AppCache,
                CleanCat::DevCache
            ]
        );
        // Back: the System page again.
        assert!(s.apply(Event::Click(Hit::CleanBack), t0).is_empty());
        assert!(s.cleaner().is_none());
        assert_eq!(s.view(), View::Panel(Page::System));
        // Reopened, it starts from the default ticks.
        s.apply(Event::Click(Hit::SystemClean), t0);
        assert_eq!(s.cleaner().unwrap().picked(), default_picks());
        // Closing the panel closes it, like another page.
        leave(&mut s, t0);
        assert!(s.cleaner().is_none());
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::System));
        assert!(s.cleaner().is_none());
        // So does scrolling to another page.
        s.apply(Event::Click(Hit::SystemClean), t0);
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Devices));
        assert!(s.cleaner().is_none());
        // Cleaner clicks off the cleaner do nothing.
        for hit in [
            Hit::CleanGo,
            Hit::CleanBack,
            Hit::CleanToggle(CleanCat::TempLogs),
        ] {
            assert!(s.apply(Event::Click(hit), t0).is_empty());
        }
        assert!(s.cleaner().is_none());
        // Scan and clean results mean nothing to the reducer.
        let scan = crate::clean::CleanEvent::Scanned(Default::default());
        assert!(s.apply(Event::Clean(scan), t0).is_empty());
    }

    #[test]
    fn a_single_click_never_cleans() {
        use crate::clean::CleanCat;
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut s = cleaner_open(t0);
        // The first click only arms the button.
        assert!(s.apply(Event::Click(Hit::CleanGo), t0).is_empty());
        assert!(s.cleaner().unwrap().armed());
        assert_eq!(s.next_deadline(), Some(t0 + CLEAN_CONFIRM));
        assert!(s.apply(Event::Click(Hit::Body), t0).is_empty());
        assert!(!s.cleaner().unwrap().armed());
        // Any other click disarms it: a tick, the body, the strip.
        for other in [Hit::CleanToggle(CleanCat::DevCache), Hit::Body, Hit::Strip] {
            s.apply(Event::Click(Hit::CleanGo), ms(10));
            assert!(s.cleaner().unwrap().armed());
            assert!(s.apply(Event::Click(other.clone()), ms(20)).is_empty());
            assert!(!s.cleaner().unwrap().armed(), "{other:?}");
            // The next click on the button arms again, never cleans.
            assert!(s.apply(Event::Click(Hit::CleanGo), ms(30)).is_empty());
            assert!(s.apply(Event::Click(Hit::Body), ms(40)).is_empty());
        }
        // The timeout disarms it (a one-shot deadline, then the Tick).
        s.apply(Event::Click(Hit::CleanGo), ms(100));
        let deadline = ms(100) + CLEAN_CONFIRM;
        assert_eq!(s.next_deadline(), Some(deadline));
        s.apply(Event::Tick, deadline);
        assert!(!s.cleaner().unwrap().armed());
        assert_eq!(s.next_deadline(), None);
        // A second click exactly at the deadline is too late: it arms again.
        let late = ms(200) + CLEAN_CONFIRM;
        s.apply(Event::Click(Hit::CleanGo), ms(200));
        assert!(s.apply(Event::Click(Hit::CleanGo), late).is_empty());
        assert!(s.cleaner().unwrap().armed());
        // Closing the panel disarms (and closes) it.
        leave(&mut s, late);
        assert!(s.cleaner().is_none());
        assert_eq!(s.next_deadline(), None);
        // Nothing ticked: the button does nothing at all.
        let mut s = cleaner_open(t0);
        for cat in default_picks() {
            s.apply(Event::Click(Hit::CleanToggle(cat)), t0);
        }
        assert!(s.apply(Event::Click(Hit::CleanGo), t0).is_empty());
        assert!(s.apply(Event::Click(Hit::CleanGo), ms(1)).is_empty());
        assert!(!s.cleaner().unwrap().armed());
    }

    #[test]
    fn two_clicks_clean_the_ticked_rows_once() {
        use crate::clean::CleanCat;
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut s = cleaner_open(t0);
        s.apply(Event::Click(Hit::CleanToggle(CleanCat::OldInstallers)), t0);
        assert!(s.apply(Event::Click(Hit::CleanGo), ms(10)).is_empty());
        let mut picks = default_picks();
        picks.push(CleanCat::OldInstallers);
        assert_eq!(
            s.apply(Event::Click(Hit::CleanGo), ms(900)),
            vec![Effect::CleanDelete(picks)]
        );
        assert!(s.clean_busy());
        assert!(!s.cleaner().unwrap().armed());
        // While it runs: no ticks, no second clean.
        s.apply(Event::Click(Hit::CleanToggle(CleanCat::TempLogs)), ms(950));
        assert!(s.cleaner().unwrap().checked[CleanCat::TempLogs.index()]);
        for n in [1000, 1100, 1200] {
            assert!(s.apply(Event::Click(Hit::CleanGo), ms(n)).is_empty());
        }
        // It may be closed meanwhile; reopened it still waits for the end.
        s.apply(Event::Click(Hit::CleanBack), ms(1300));
        s.apply(Event::Click(Hit::SystemClean), ms(1400));
        assert!(s.clean_busy());
        s.apply(Event::CleanFinished, ms(5000));
        assert!(!s.clean_busy());
        // Afterwards the same two clicks clean again.
        s.apply(Event::Click(Hit::CleanGo), ms(5100));
        assert_eq!(
            s.apply(Event::Click(Hit::CleanGo), ms(5200)),
            vec![Effect::CleanDelete(default_picks())]
        );
    }

    /// The cleaner switched to "Otomatis · Mole", Mole found: its dry-run
    /// with the default options was asked for.
    fn mole_scanning(now: Instant) -> IslandState {
        let mut s = cleaner_open(now);
        assert_eq!(
            s.apply(Event::Click(Hit::CleanMode(CleanMode::Mole)), now),
            vec![Effect::SaveCleanMode(CleanMode::Mole), Effect::MoleDetect]
        );
        assert_eq!(s.mole_phase(), MolePhase::Detecting);
        assert_eq!(
            s.apply(Event::MoleFound(true), now),
            vec![Effect::MolePreview(MoleOpts::default())]
        );
        assert_eq!(s.mole_phase(), MolePhase::Scanning(MoleOpts::default()));
        s
    }

    /// Every effect that would make Mole delete something.
    fn cleans(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|e| matches!(e, Effect::MoleClean(_) | Effect::CleanDelete(_)))
            .count()
    }

    #[test]
    fn mole_mode_is_remembered_and_looks_for_mole() {
        let t0 = Instant::now();
        let mut s = mole_scanning(t0);
        assert_eq!(s.clean_mode(), CleanMode::Mole);
        assert_eq!(s.cleaner().unwrap().mode, CleanMode::Mole);
        // Own mode's clicks do nothing in Mole mode.
        assert!(s.apply(Event::Click(Hit::CleanGo), t0).is_empty());
        assert!(!s.cleaner().unwrap().armed());
        // Back to the own mode: the dry-run stops, its scan runs again.
        let effects = s.apply(Event::Click(Hit::CleanMode(CleanMode::Own)), t0);
        assert_eq!(
            effects,
            vec![
                Effect::SaveCleanMode(CleanMode::Own),
                Effect::OpenCleaner,
                Effect::MoleStopPreview
            ]
        );
        assert_eq!(s.mole_phase(), MolePhase::Unknown);
        // The remembered mode at startup opens the cleaner in it.
        let mut s = st();
        s.apply(Event::CleanModeSetting(CleanMode::Mole), t0);
        open_on(&mut s, Page::System, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::SystemClean), t0),
            vec![Effect::MoleDetect]
        );
        // Not installed: the card; "Cara memasang" opens the README and
        // gets the panel out of the way; nothing else takes a click.
        assert!(s.apply(Event::MoleFound(false), t0).is_empty());
        assert_eq!(s.mole_phase(), MolePhase::Missing);
        for hit in [
            Hit::MoleGo,
            Hit::MoleProtect,
            Hit::MoleOpt(MoleOpt::System),
            Hit::MoleShowLog,
        ] {
            assert!(s.apply(Event::Click(hit.clone()), t0).is_empty(), "{hit:?}");
        }
        assert_eq!(s.mole_opts(), MoleOpts::default());
        assert_eq!(
            s.apply(Event::Click(Hit::MoleInstall), t0),
            vec![Effect::OpenMoleInstall]
        );
        assert_eq!(s.view(), View::Idle);
        // A late answer after the cleaner closed starts nothing.
        let mut s = mole_scanning(t0);
        s.apply(Event::Click(Hit::CleanBack), t0);
        assert!(s.apply(Event::MoleFound(true), t0).is_empty());
    }

    #[test]
    fn mole_never_cleans_without_a_preview_and_a_confirm() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut s = mole_scanning(t0);
        let opts = MoleOpts::default();
        // While the dry-run runs the button neither arms nor cleans.
        for n in [1, 2, 3] {
            assert_eq!(cleans(&s.apply(Event::Click(Hit::MoleGo), ms(n))), 0);
            assert!(!s.cleaner().unwrap().armed());
        }
        // A preview that failed, or one for other options: still nothing.
        let system = MoleOpts {
            system: true,
            ..opts
        };
        s.apply(
            Event::MolePreviewed {
                opts: system,
                ok: true,
            },
            ms(5),
        );
        assert_eq!(s.mole_phase(), MolePhase::Scanning(opts), "not this run's");
        s.apply(Event::MolePreviewed { opts, ok: false }, ms(6));
        assert_eq!(s.mole_phase(), MolePhase::Previewed { opts, ok: false });
        for n in [7, 8, 9] {
            assert_eq!(cleans(&s.apply(Event::Click(Hit::MoleGo), ms(n))), 0);
            assert!(!s.cleaner().unwrap().armed());
        }
        // "Pratinjau lagi" is the way on: an option change previews again.
        assert_eq!(
            s.apply(Event::Click(Hit::MoleOpt(MoleOpt::GameMedia)), ms(10)),
            vec![Effect::MolePreview(MoleOpts {
                game_media: true,
                ..opts
            })]
        );
        let opts = s.mole_opts();
        assert!(
            opts.game_media && !opts.system,
            "admin is never ticked by itself"
        );
        s.apply(Event::MolePreviewed { opts, ok: true }, ms(20));
        // The first click arms ("Yakin? Hapus permanen"), never cleans.
        assert!(s.apply(Event::Click(Hit::MoleGo), ms(30)).is_empty());
        assert!(s.cleaner().unwrap().armed());
        assert_eq!(s.next_deadline(), Some(ms(30) + CLEAN_CONFIRM));
        assert_eq!(cleans(&s.apply(Event::Click(Hit::Body), ms(35))), 0);
        assert!(!s.cleaner().unwrap().armed());
        // Any other click disarms it; so does an option, which also
        // throws the preview away.
        for other in [Hit::Body, Hit::Strip, Hit::MoleProtect, Hit::MoleShowLog] {
            s.apply(Event::Click(Hit::MoleGo), ms(40));
            assert!(s.cleaner().unwrap().armed());
            assert_eq!(cleans(&s.apply(Event::Click(other.clone()), ms(41))), 0);
            assert!(!s.cleaner().unwrap().armed(), "{other:?}");
            // The next click arms again; it never cleans.
            assert_eq!(cleans(&s.apply(Event::Click(Hit::MoleGo), ms(42))), 0);
            s.apply(Event::Click(Hit::Body), ms(43));
            if s.cleaner().unwrap().mole_log {
                s.apply(Event::Click(Hit::MoleShowLog), ms(44));
            }
        }
        // The timeout disarms it.
        s.apply(Event::Click(Hit::MoleGo), ms(100));
        s.apply(Event::Tick, ms(100) + CLEAN_CONFIRM);
        assert!(!s.cleaner().unwrap().armed());
        assert_eq!(
            cleans(&s.apply(Event::Click(Hit::MoleGo), ms(100) + CLEAN_CONFIRM)),
            0
        );
        s.apply(Event::Click(Hit::Body), ms(5000));
        // An option ticked while armed: disarmed, a new dry-run, no clean.
        s.apply(Event::Click(Hit::MoleGo), ms(5100));
        let effects = s.apply(Event::Click(Hit::MoleOpt(MoleOpt::System)), ms(5200));
        let both = MoleOpts {
            system: true,
            game_media: true,
        };
        assert_eq!(effects, vec![Effect::MolePreview(both)]);
        assert!(!s.cleaner().unwrap().armed());
        assert_eq!(cleans(&s.apply(Event::Click(Hit::MoleGo), ms(5300))), 0);
        assert_eq!(cleans(&s.apply(Event::Click(Hit::MoleGo), ms(5400))), 0);
        // Its preview done, two clicks inside CLEAN_CONFIRM clean once,
        // with the previewed options.
        s.apply(
            Event::MolePreviewed {
                opts: both,
                ok: true,
            },
            ms(9000),
        );
        assert!(s.apply(Event::Click(Hit::MoleGo), ms(9100)).is_empty());
        assert_eq!(
            s.apply(Event::Click(Hit::MoleGo), ms(9200)),
            vec![Effect::MoleClean(both)]
        );
        assert_eq!(s.mole_phase(), MolePhase::Running);
        // While it runs: no second clean, no options, no mode switch, and
        // the own mode's button waits too.
        for hit in [
            Hit::MoleGo,
            Hit::MoleGo,
            Hit::MoleOpt(MoleOpt::GameMedia),
            Hit::CleanMode(CleanMode::Own),
            Hit::MoleProtect,
        ] {
            assert!(
                s.apply(Event::Click(hit.clone()), ms(9300)).is_empty(),
                "{hit:?}"
            );
        }
        assert_eq!(s.mole_opts(), both);
        // The panel may close: the clean goes on (no stop), and the
        // cleaner reopens on it.
        assert!(s.apply(Event::Hover(false), ms(9400)).is_empty());
        assert_eq!(s.mole_phase(), MolePhase::Running);
        s.apply(Event::Hover(true), ms(9500));
        assert!(s.apply(Event::Click(Hit::SystemClean), ms(9600)).is_empty());
        assert_eq!(s.cleaner().unwrap().mode, CleanMode::Mole);
        s.apply(Event::MoleFinished { ok: true }, ms(20_000));
        assert_eq!(s.mole_phase(), MolePhase::Finished { ok: true });
        // Afterwards the button previews again; it never cleans from here.
        assert_eq!(
            s.apply(Event::Click(Hit::MoleGo), ms(20_100)),
            vec![Effect::MolePreview(both)]
        );
        assert_eq!(cleans(&s.apply(Event::Click(Hit::MoleGo), ms(20_200))), 0);
    }

    #[test]
    fn closing_the_cleaner_stops_a_mole_preview() {
        let t0 = Instant::now();
        let mut s = mole_scanning(t0);
        assert_eq!(leave(&mut s, t0), vec![Effect::MoleStopPreview]);
        assert_eq!(s.mole_phase(), MolePhase::Unknown);
        // Its late answer is not a preview.
        s.apply(
            Event::MolePreviewed {
                opts: MoleOpts::default(),
                ok: true,
            },
            t0,
        );
        assert_eq!(s.mole_phase(), MolePhase::Unknown);
        // Another page does the same; a finished preview is forgotten.
        let mut s = mole_scanning(t0);
        s.apply(
            Event::MolePreviewed {
                opts: MoleOpts::default(),
                ok: true,
            },
            t0,
        );
        s.apply(Event::Click(Hit::MoleGo), t0);
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.mole_phase(), MolePhase::Unknown);
        // Back on the cleaner, Mole is looked for and previewed anew: the
        // old arming is gone.
        s.apply(Event::Wheel(120), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::SystemClean), t0),
            vec![Effect::MoleDetect]
        );
        assert!(!s.cleaner().unwrap().armed());
        assert_eq!(cleans(&s.apply(Event::Click(Hit::MoleGo), t0)), 0);
    }

    fn mole_notice(kind: NoticeKind) -> Notice {
        Notice {
            key: "mole".into(),
            source: "Bersihkan".into(),
            title: "Mole".into(),
            body: String::new(),
            kind,
        }
    }

    #[test]
    fn mole_notices_open_its_log() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        // A finished clean with the panel shut: the notice is temporary.
        let mut s = mole_scanning(t0);
        s.apply(
            Event::MolePreviewed {
                opts: MoleOpts::default(),
                ok: true,
            },
            t0,
        );
        s.apply(Event::Click(Hit::MoleGo), ms(1));
        s.apply(Event::Click(Hit::MoleGo), ms(2));
        leave(&mut s, ms(3));
        s.apply(Event::MoleFinished { ok: true }, ms(4));
        let done = mole_notice(NoticeKind::MoleDone);
        s.apply(Event::Notice(done.clone()), ms(4));
        assert_eq!(s.view(), View::Notice(done));
        assert_eq!(s.next_deadline(), Some(ms(4) + NOTICE_DURATION));
        // Its click opens the cleaner on Mole's log, the result kept.
        assert!(s.apply(Event::Click(Hit::Body), ms(5)).is_empty());
        assert!(s.apply(Event::Hover(true), ms(6)).is_empty());
        assert_eq!(s.view(), View::Panel(Page::System));
        assert_eq!(s.cleaner().unwrap().mode, CleanMode::Mole);
        assert_eq!(s.mole_phase(), MolePhase::Finished { ok: true });
        // A failed one stays until clicked (sticky), and opens the log too.
        let mut s = st();
        let failed = mole_notice(NoticeKind::MoleFailed);
        s.apply(Event::Notice(failed.clone()), t0);
        assert_eq!(s.view(), View::Notice(failed.clone()));
        assert_eq!(s.next_deadline(), None);
        s.apply(Event::Notice(failed.clone()), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.waiting_strip(), Some(&failed), "one at a time");
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::System));
        assert_eq!(s.cleaner().unwrap().mode, CleanMode::Mole);
        assert_eq!(s.waiting_strip(), None);
    }

    #[test]
    fn a_go_tile_switches_the_page_without_collapsing() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Shortcuts, t0);
        for page in [Page::Shelf, Page::Calendar, Page::Cli, Page::Mixer] {
            let effects = s.apply(Event::Click(Hit::Shortcut(Shortcut::Go(page))), t0);
            // The new page's own refresh may come; never a shortcut run.
            assert!(
                effects
                    .iter()
                    .all(|e| matches!(e, Effect::RefreshCalendar { .. })),
                "{page:?}: {effects:?}"
            );
            assert_eq!(s.view(), View::Panel(page), "no collapse");
        }
    }

    #[test]
    fn a_go_tile_opens_a_page_switched_off_for_that_visit_only() {
        let t0 = Instant::now();
        let mut s = st();
        let mut pages = PanelPages::default();
        pages.toggle(Page::Notes);
        s.apply(Event::PanelPages(pages), t0);
        open_on(&mut s, Page::Shortcuts, t0);
        let effects = s.apply(Event::Click(Hit::Shortcut(Shortcut::Go(Page::Notes))), t0);
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(s.view(), View::Panel(Page::Notes));
        // The click lingers the panel; once that has run out and the
        // pointer is away it closes, and the next visit starts on a page
        // that is on.
        let later = t0 + ACTIVITY_LINGER + Duration::from_secs(1);
        s.apply(Event::Hover(false), later);
        s.apply(Event::Hover(true), later);
        assert_eq!(s.view(), View::Panel(Page::Media));
    }

    #[test]
    fn the_vale_settings_tile_collapses_and_asks_to_open_the_settings() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Shortcuts, t0);
        let effects = s.apply(Event::Click(Hit::Shortcut(Shortcut::ValeSettings)), t0);
        assert_eq!(effects, vec![Effect::OpenSettings]);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn the_clipboard_tile_opens_the_clipboard_page() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Shortcuts, t0);
        let effects = s.apply(Event::Click(Hit::Shortcut(Shortcut::Clipboard)), t0);
        // No Win+V, no collapse: the island's own page.
        assert_eq!(effects, vec![]);
        assert_eq!(s.view(), View::Panel(Page::Clipboard));
        // The page sits between Mixer and the shelf, then Sistem, Perangkat
        // and CLI.
        s.apply(Event::Wheel(120), t0);
        assert_eq!(s.view(), View::Panel(Page::Mixer));
        s.apply(Event::Wheel(-240), t0);
        assert_eq!(s.view(), View::Panel(Page::Notifications));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::System));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Devices));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Cli));
    }

    #[test]
    fn clipboard_hits_act_only_on_their_page() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        for hit in [Hit::ClipSearch, Hit::ClipCard(1), Hit::ClipDelete(1)] {
            assert_eq!(s.apply(Event::Click(hit), t0), vec![]);
        }
        open_on(&mut s, Page::Clipboard, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::ClipSearch), t0),
            vec![Effect::FocusClipSearch]
        );
        let cases = [
            (Hit::ClipCard(3), ClipAction::Copy(3)),
            (Hit::ClipPin(3), ClipAction::TogglePin(3)),
            (Hit::ClipPlain(3), ClipAction::CopyPlain(3)),
            (Hit::ClipDelete(3), ClipAction::Delete(3)),
        ];
        for (hit, action) in cases {
            assert_eq!(s.apply(Event::Click(hit), t0), vec![Effect::Clip(action)]);
        }
    }

    #[test]
    fn the_search_field_holds_the_panel_and_lets_go_with_its_page() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Clipboard, t0);
        s.apply(Event::ClipFocus(true), t0);
        assert_eq!(s.apply(Event::Hover(false), t0), vec![]);
        assert_eq!(
            s.view(),
            View::Panel(Page::Clipboard),
            "held by the keyboard"
        );
        // Enter copied: the panel closes and the keyboard goes back.
        let effects = s.apply(Event::ClipCommit, t0);
        assert_eq!(effects, vec![Effect::ReleaseClipFocus]);
        assert!(!s.clip_focused());
        assert_eq!(s.view(), View::Idle);
        // A page change lets go too.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Clipboard));
        s.apply(Event::ClipFocus(true), t0);
        assert_eq!(
            s.apply(Event::Wheel(-120), t0),
            vec![Effect::ReleaseClipFocus]
        );
        assert_eq!(s.view(), View::Panel(Page::Notifications));
    }

    #[test]
    fn hapus_semua_arms_then_clears_on_a_second_click_inside_the_confirm_time() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut s = st();
        open_on(&mut s, Page::Notifications, t0);
        assert!(!s.notif_armed());
        // The first click only arms: no effect, a deadline on the one timer.
        assert_eq!(s.apply(Event::Click(Hit::NotifClear), t0), vec![]);
        assert!(s.notif_armed());
        assert_eq!(s.next_deadline(), Some(t0 + CLEAN_CONFIRM));
        // A second one inside it clears, and disarms.
        assert_eq!(
            s.apply(Event::Click(Hit::NotifClear), ms(1000)),
            vec![Effect::Notif(NotifAction::ClearAll)]
        );
        assert!(!s.notif_armed());
        // Armed and left alone, it turns back when the time is up.
        s.apply(Event::Click(Hit::NotifClear), ms(2000));
        assert!(s.notif_armed());
        s.apply(
            Event::Tick,
            ms(2000) + CLEAN_CONFIRM + Duration::from_millis(1),
        );
        assert!(!s.notif_armed(), "expired");
        // A click after it expired arms again instead of clearing.
        let late = ms(2000) + CLEAN_CONFIRM + Duration::from_millis(5);
        assert_eq!(s.apply(Event::Click(Hit::NotifClear), late), vec![]);
        assert!(s.notif_armed());
    }

    #[test]
    fn hapus_semua_disarms_on_any_other_click_a_page_change_and_the_panel_closing() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notifications, t0);
        s.apply(Event::Click(Hit::NotifClear), t0);
        assert!(s.notif_armed());
        // Another button of the page: it acts and the first click is spent.
        assert_eq!(
            s.apply(Event::Click(Hit::NotifPause), t0),
            vec![Effect::Notif(NotifAction::TogglePause)]
        );
        assert!(!s.notif_armed());
        assert_eq!(
            s.apply(Event::Click(Hit::NotifClear), t0),
            vec![],
            "so the next click only arms again"
        );
        // A page change.
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        assert!(!s.notif_armed());
        // The panel closing (the linger, like the confirm time, is four
        // seconds, so this shows the two end together, not which came first).
        s.apply(Event::Wheel(120), t0);
        s.apply(Event::Click(Hit::NotifClear), t0);
        assert!(s.notif_armed());
        s.apply(Event::Hover(false), t0);
        let later = t0 + ACTIVITY_LINGER + Duration::from_millis(1);
        s.apply(Event::Tick, later);
        assert_eq!(s.view(), View::Idle);
        assert!(!s.notif_armed());
    }

    #[test]
    fn the_notifikasi_buttons_do_nothing_when_the_page_is_not_showing() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Media, t0);
        for hit in [
            Hit::NotifRow(1),
            Hit::NotifOpen(1),
            Hit::NotifMute(1),
            Hit::NotifRemove(1),
            Hit::NotifPause,
            Hit::NotifClear,
            Hit::NotifFocus,
            Hit::NotifSettings,
        ] {
            assert_eq!(s.apply(Event::Click(hit.clone()), t0), vec![], "{hit:?}");
        }
        assert!(!s.notif_armed());
    }

    #[test]
    fn rows_and_buttons_ask_for_their_action_and_the_background_asks_for_the_keyboard() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notifications, t0);
        let fx = |hit: Hit, s: &mut IslandState| s.apply(Event::Click(hit), t0);
        // A row and its Buka both open (mark read, bring the app forward).
        assert_eq!(
            fx(Hit::NotifRow(4), &mut s),
            vec![Effect::Notif(NotifAction::Open(4))]
        );
        assert_eq!(
            fx(Hit::NotifOpen(4), &mut s),
            vec![Effect::Notif(NotifAction::Open(4))]
        );
        assert_eq!(
            fx(Hit::NotifMute(4), &mut s),
            vec![Effect::Notif(NotifAction::Mute(4))]
        );
        assert_eq!(
            fx(Hit::NotifRemove(4), &mut s),
            vec![Effect::Notif(NotifAction::Remove(4))]
        );
        assert_eq!(
            fx(Hit::NotifSettings, &mut s),
            vec![Effect::Notif(NotifAction::OpenSettings)]
        );
        // The background asks to focus the page's input; the island only
        // activates for it when `App` says so (it sends `NotifFocus(true)`).
        assert_eq!(fx(Hit::NotifFocus, &mut s), vec![Effect::FocusNotifInput]);
        assert!(!s.notif_focused());
    }

    #[test]
    fn the_notifikasi_input_holds_the_panel_and_lets_go_with_its_page_or_a_collapse() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notifications, t0);
        s.apply(Event::NotifFocus(true), t0);
        assert!(s.notif_focused());
        // The pointer leaves: the keyboard still holds the panel open.
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Panel(Page::Notifications));
        // The keyboard going away lets the panel close with the pointer away.
        s.apply(Event::NotifFocus(false), t0);
        assert_eq!(s.view(), View::Idle);
        // A page change lets go (the keyboard goes back to its window).
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Notifications));
        s.apply(Event::NotifFocus(true), t0);
        assert_eq!(
            s.apply(Event::Wheel(-120), t0),
            vec![Effect::ReleaseNotifFocus]
        );
        assert!(!s.notif_focused());
        // Another page's input takes over from it: the search field.
        s.apply(Event::Wheel(120), t0);
        s.apply(Event::NotifFocus(true), t0);
        s.apply(Event::SearchHotkey, t0);
        assert!(!s.notif_focused(), "the quick search has the keyboard now");
    }

    #[test]
    fn the_hotkey_opens_the_page_without_hover_and_closes_it_again() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(s.apply(Event::ClipHotkey, t0), vec![]);
        assert_eq!(s.view(), View::Panel(Page::Clipboard));
        assert!(s.clip_held());
        // Pressed again: closed.
        s.apply(Event::ClipHotkey, t0);
        assert_eq!(s.view(), View::Idle);
        // Not visited in time: closed.
        s.apply(Event::ClipHotkey, t0);
        s.apply(Event::ClipHoldEnd, t0);
        assert_eq!(s.view(), View::Idle);
        // Visited: hover holds it, leaving closes it.
        s.apply(Event::ClipHotkey, t0);
        s.apply(Event::Hover(true), t0);
        assert!(!s.clip_held());
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Idle);
        // Never over a fullscreen app.
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::ClipHotkey, t0);
        assert!(!s.clip_held());
    }

    #[test]
    fn the_devices_hotkey_works_like_the_clipboards_and_the_holds_do_not_mix() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(s.apply(Event::DevicesHotkey, t0), vec![]);
        assert_eq!(s.view(), View::Panel(Page::Devices));
        assert!(s.devices_held() && s.hotkey_held() && !s.clip_held());
        // Pressed again while it shows: closed.
        s.apply(Event::DevicesHotkey, t0);
        assert_eq!(s.view(), View::Idle);
        assert!(!s.hotkey_held());
        // Not visited in time: closed (the one hold timer ends either).
        s.apply(Event::DevicesHotkey, t0);
        s.apply(Event::ClipHoldEnd, t0);
        assert_eq!(s.view(), View::Idle);
        assert!(!s.devices_held());
        // Visited: hover holds it, leaving closes it.
        s.apply(Event::DevicesHotkey, t0);
        s.apply(Event::Hover(true), t0);
        assert!(!s.devices_held());
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Idle);
        // The other hotkey takes the panel over: one hold, the Clipboard's.
        s.apply(Event::DevicesHotkey, t0);
        s.apply(Event::ClipHotkey, t0);
        assert_eq!(s.view(), View::Panel(Page::Clipboard));
        assert!(s.clip_held() && !s.devices_held());
        s.apply(Event::DevicesHotkey, t0);
        assert_eq!(s.view(), View::Panel(Page::Devices));
        assert!(s.devices_held() && !s.clip_held());
        // A page change ends the hold (the pointer comes and turns the page).
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Wheel(-120), t0);
        assert!(!s.hotkey_held());
        s.apply(Event::Hover(false), t0);
        // A tile that collapses the panel ends it too.
        s.apply(Event::DevicesHotkey, t0);
        assert!(s.devices_held());
        s.apply(Event::Click(Hit::DeviceOpen(0)), t0);
        assert!(!s.devices_held());
        // Never over a fullscreen app.
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::DevicesHotkey, t0);
        assert!(!s.devices_held());
    }

    #[test]
    fn the_arrange_hotkey_opens_the_page_held_and_toggles_it() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(s.apply(Event::ArrangeHotkey, t0), vec![]);
        assert_eq!(s.view(), View::Panel(Page::Arrange));
        assert!(s.hotkey_held());
        // Pressed again while it shows: closed.
        s.apply(Event::ArrangeHotkey, t0);
        assert_eq!(s.view(), View::Idle);
        assert!(!s.hotkey_held());
        // Not visited in time: closed by the shared hold timer.
        s.apply(Event::ArrangeHotkey, t0);
        s.apply(Event::ClipHoldEnd, t0);
        assert_eq!(s.view(), View::Idle);
        // Another page's hotkey takes the panel over.
        s.apply(Event::ArrangeHotkey, t0);
        s.apply(Event::DevicesHotkey, t0);
        assert_eq!(s.view(), View::Panel(Page::Devices));
        // Never over a fullscreen app.
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::ArrangeHotkey, t0);
        assert!(!s.hotkey_held());
    }

    #[test]
    fn the_susun_controls_count_only_while_the_page_shows() {
        let t0 = Instant::now();
        let mut s = st();
        let hits = [
            (Hit::ArrangeMonitor(1), Effect::ArrangeMonitor(1)),
            (Hit::ArrangeCard(2), Effect::ArrangeCard(2)),
            (Hit::ArrangePlay(0), Effect::ArrangePlay(0)),
            (Hit::ArrangeWindow(3), Effect::ArrangeWindow(3)),
            (Hit::ArrangeSkip, Effect::ArrangeSkip),
        ];
        for (hit, _) in &hits {
            assert_eq!(s.apply(Event::Click(hit.clone()), t0), vec![], "{hit:?}");
        }
        open_on(&mut s, Page::Calendar, t0);
        for (hit, _) in &hits {
            assert_eq!(s.apply(Event::Click(hit.clone()), t0), vec![], "{hit:?}");
        }
        open_on(&mut s, Page::Arrange, t0);
        for (hit, effect) in hits {
            assert_eq!(s.apply(Event::Click(hit), t0), vec![effect]);
        }
        assert_eq!(s.view(), View::Panel(Page::Arrange), "the panel stays");
    }

    #[test]
    fn the_pick_step_keeps_the_panel_open_in_every_mode_and_ignores_a_click_outside() {
        let t0 = Instant::now();
        for mode in [OpenMode::Hover, OpenMode::HoverStay, OpenMode::Click] {
            let mut s = st();
            s.apply(Event::OpenMode(mode), t0);
            s.apply(Event::ArrangeHotkey, t0);
            s.apply(Event::Hover(true), t0);
            s.apply(Event::ArrangePick(true), t0);
            // The pointer goes to the overlay, and a click lands there.
            s.apply(Event::Hover(false), t0);
            s.apply(Event::Dismiss, t0);
            assert_eq!(s.view(), View::Panel(Page::Arrange), "{mode:?}");
            // Over: the panel follows the usual rules again.
            s.apply(Event::ArrangePick(false), t0);
            leave(&mut s, t0);
            assert_eq!(s.view(), View::Idle, "{mode:?}");
        }
    }

    fn arranged(save: ArrangeSave) -> Notice {
        Notice {
            key: "arrange-done".into(),
            source: "Susun".into(),
            title: "Tersusun \u{b7} 3 jendela".into(),
            body: "3 kolom".into(),
            kind: NoticeKind::Arranged { save },
        }
    }

    #[test]
    fn the_arranged_notice_lasts_three_seconds_and_waits_for_the_name_field() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(arranged(ArrangeSave::Offer)), t0);
        assert!(
            matches!(s.view(), View::Notice(n) if matches!(n.kind, NoticeKind::Arranged { .. }))
        );
        assert_eq!(s.next_deadline(), Some(t0 + ARRANGE_NOTICE_DURATION));
        assert_eq!(ARRANGE_NOTICE_DURATION, Duration::from_secs(3));
        // The name field up: it does not run out.
        s.apply(Event::ArrangeNaming(true), t0);
        assert_eq!(s.next_deadline(), None);
        s.apply(Event::Tick, t0 + Duration::from_secs(30));
        assert!(matches!(s.view(), View::Notice(_)), "still there");
        // The field gone: it goes at the next tick.
        s.apply(Event::ArrangeNaming(false), t0 + Duration::from_secs(30));
        s.apply(Event::Tick, t0 + Duration::from_secs(31));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn batalkan_still_undoes_while_the_name_field_is_up_past_the_three_seconds() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(arranged(ArrangeSave::Offer)), t0);
        s.apply(Event::Click(Hit::ArrangeSave), t0);
        s.apply(Event::ArrangeNaming(true), t0);
        let later = t0 + Duration::from_secs(10);
        s.apply(Event::Tick, later);
        assert_eq!(
            s.apply(Event::Click(Hit::ArrangeUndo), later),
            vec![Effect::ArrangeUndo]
        );
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn the_arranged_buttons_undo_and_save_only_from_the_notice() {
        let t0 = Instant::now();
        let mut s = st();
        // No notice: nothing.
        assert_eq!(s.apply(Event::Click(Hit::ArrangeUndo), t0), vec![]);
        assert_eq!(s.apply(Event::Click(Hit::ArrangeSave), t0), vec![]);
        // Batalkan: the undo runs and the notice closes.
        s.apply(Event::Notice(arranged(ArrangeSave::Offer)), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::ArrangeUndo), t0),
            vec![Effect::ArrangeUndo]
        );
        assert_eq!(s.view(), View::Idle);
        // Simpan...: the notice stays for the name field.
        s.apply(Event::Notice(arranged(ArrangeSave::Offer)), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::ArrangeSave), t0),
            vec![Effect::ArrangeSave]
        );
        assert!(matches!(s.view(), View::Notice(_)));
        // A saved run has no Simpan; a full list disables it.
        for save in [ArrangeSave::None, ArrangeSave::Full] {
            let mut s = st();
            s.apply(Event::Notice(arranged(save)), t0);
            assert_eq!(
                s.apply(Event::Click(Hit::ArrangeSave), t0),
                vec![],
                "{save:?}"
            );
        }
    }

    #[test]
    fn the_devices_buttons_count_only_while_the_page_shows() {
        let t0 = Instant::now();
        let mut s = st();
        // Closed, or on another page: nothing.
        for hit in [
            Hit::DeviceEject(0),
            Hit::DeviceRetry(0),
            Hit::DeviceDetail(0),
            Hit::DeviceOpen(0),
            Hit::DevicesEjectAll,
        ] {
            assert_eq!(s.apply(Event::Click(hit.clone()), t0), vec![], "{hit:?}");
        }
        open_on(&mut s, Page::Calendar, t0);
        assert_eq!(s.apply(Event::Click(Hit::DeviceEject(0)), t0), vec![]);
        open_on(&mut s, Page::Devices, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::DeviceEject(2)), t0),
            vec![Effect::DeviceEject(2)]
        );
        // "Coba lagi" asks for the same eject.
        assert_eq!(
            s.apply(Event::Click(Hit::DeviceRetry(1)), t0),
            vec![Effect::DeviceEject(1)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::DeviceDetail(0)), t0),
            vec![Effect::DeviceDetail(0)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::DevicesEjectAll), t0),
            vec![Effect::DevicesEjectAll]
        );
        // The panel stays for those.
        assert_eq!(s.view(), View::Panel(Page::Devices));
        // The folder button gets the panel out of the way, like a drive's meter.
        assert_eq!(
            s.apply(Event::Click(Hit::DeviceOpen(1)), t0),
            vec![Effect::DeviceOpen(1)]
        );
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn an_action_on_the_devices_page_keeps_the_panel_open_a_while() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Devices, t0);
        s.apply(Event::Click(Hit::DeviceDetail(0)), t0);
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Panel(Page::Devices), "the linger holds it");
        s.apply(Event::Tick, t0 + ACTIVITY_LINGER + secs(1));
        assert_eq!(s.view(), View::Idle);
    }

    fn plugged(letter: char) -> Notice {
        Notice {
            key: "device-plugged-k".into(),
            source: "Perangkat".into(),
            title: "SanDisk Ultra tersambung".into(),
            body: "E: · 38,9 GB kosong".into(),
            kind: NoticeKind::DevicePlugged { letter },
        }
    }

    fn safe() -> Notice {
        Notice {
            key: "device-safe".into(),
            source: "Perangkat".into(),
            title: "Aman dicabut".into(),
            body: "SanDisk Ultra".into(),
            kind: NoticeKind::DeviceSafe,
        }
    }

    #[test]
    fn a_plugged_notice_has_a_button_and_its_click_opens_the_drive() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(plugged('E')), t0);
        assert_eq!(s.view(), View::Notice(plugged('E')));
        // Temporary, like the battery's.
        assert_eq!(s.next_deadline(), Some(t0 + NOTICE_DURATION));
        // It has a button: hover does not open the panel over it.
        assert!(notice_has_action(&plugged('E')));
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(plugged('E')));
        // The click dismisses it, opens Explorer on the drive and keeps the
        // panel shut until the pointer leaves and comes back.
        let effects = s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(effects, vec![Effect::OpenDrive('E')]);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        // A letter that is not one opens nothing.
        let mut s = st();
        s.apply(Event::Notice(plugged('\0')), t0);
        s.apply(Event::Hover(true), t0);
        let effects = s.apply(Event::Click(Hit::Body), t0);
        assert!(!effects.iter().any(|e| matches!(e, Effect::OpenDrive(_))));
    }

    fn update_notice(kind: NoticeKind, title: &str) -> Notice {
        Notice {
            key: "update".into(),
            source: "Vale".into(),
            title: title.into(),
            body: String::new(),
            kind,
        }
    }

    fn available() -> Notice {
        update_notice(
            NoticeKind::UpdateAvailable {
                version: "3.11.0".into(),
            },
            "Vale 3.11.0 tersedia",
        )
    }

    fn progress(pct: u8) -> Notice {
        update_notice(
            NoticeKind::UpdateProgress { pct },
            &format!("Mengunduh pembaruan\u{2026} {pct}%"),
        )
    }

    fn update_failed() -> Notice {
        update_notice(NoticeKind::UpdateFailed, "Update gagal: tidak ada koneksi")
    }

    #[test]
    fn the_update_offer_has_a_button_hover_leaves_alone_and_the_click_installs() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(available()), t0);
        assert_eq!(s.view(), View::Notice(available()));
        assert_eq!(s.next_deadline(), Some(t0 + UPDATE_NOTICE_DURATION));
        assert!(notice_has_action(&available()));
        // Hover does not open the panel over the button.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(available()));
        let effects = s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(effects, vec![Effect::InstallUpdate]);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn the_failed_update_notice_opens_the_releases_page() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(update_failed()), t0);
        assert!(notice_has_action(&update_failed()));
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(update_failed()));
        assert_eq!(
            s.apply(Event::Click(Hit::Body), t0),
            vec![Effect::OpenReleases]
        );
    }

    #[test]
    fn the_info_notice_and_the_progress_only_dismiss() {
        let t0 = Instant::now();
        for notice in [
            update_notice(NoticeKind::UpdateInfo, "Vale sudah versi terbaru (3.10.0)"),
            progress(10),
        ] {
            let mut s = st();
            s.apply(Event::Notice(notice.clone()), t0);
            assert!(!notice_has_action(&notice));
            assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![]);
            assert_eq!(s.view(), View::Idle);
        }
    }

    #[test]
    fn the_progress_is_replaced_where_it_stands_and_timed_from_its_last_update() {
        let t0 = Instant::now();
        let secs = Duration::from_secs;
        let mut s = st();
        s.apply(Event::Notice(progress(0)), t0);
        assert_eq!(s.view(), View::Notice(progress(0)));
        assert_eq!(s.next_deadline(), Some(t0 + UPDATE_PROGRESS_DURATION));
        assert_eq!(UPDATE_PROGRESS_DURATION, secs(15));
        // A newer percent replaces the current one: still one notice, and the
        // 15 s start over from this update.
        s.apply(Event::Notice(progress(40)), t0 + secs(10));
        assert_eq!(s.view(), View::Notice(progress(40)));
        assert_eq!(s.next_deadline(), Some(t0 + secs(10) + UPDATE_PROGRESS_DURATION));
        s.apply(Event::Tick, t0 + secs(20));
        assert_eq!(s.view(), View::Notice(progress(40)), "past the first 15 s");
        // Stalled for 15 s: it fades.
        s.apply(Event::Tick, t0 + secs(25));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn a_queued_progress_is_replaced_in_the_queue_not_added() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(plugged('E')), t0);
        s.apply(Event::Notice(progress(5)), t0);
        s.apply(Event::Notice(progress(6)), t0);
        s.apply(Event::Notice(progress(7)), t0);
        assert_eq!(s.view(), View::Notice(plugged('E')));
        s.apply(Event::Click(Hit::Body), t0);
        // The one queued progress, with the newest percent.
        assert_eq!(s.view(), View::Notice(progress(7)));
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn a_failure_takes_the_progress_away_shown_or_waiting() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(progress(30)), t0);
        s.apply(Event::Notice(update_failed()), t0);
        assert_eq!(s.view(), View::Notice(update_failed()));
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Idle, "no progress left behind");
        // Waiting behind another notice.
        let mut s = st();
        s.apply(Event::Notice(plugged('E')), t0);
        s.apply(Event::Notice(progress(30)), t0);
        s.apply(Event::Notice(update_failed()), t0);
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Notice(update_failed()));
    }

    #[test]
    fn the_update_events_are_ignored_by_the_reducer() {
        use crate::update::UpdateEvent;
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(s.apply(Event::Update(UpdateEvent::Progress(5)), t0), vec![]);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn the_safe_notice_is_temporary_and_a_click_only_dismisses_it() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(safe()), t0);
        assert_eq!(s.view(), View::Notice(safe()));
        assert_eq!(s.next_deadline(), Some(t0 + NOTICE_DURATION));
        assert!(!notice_has_action(&safe()), "nothing to click for");
        s.apply(Event::Hover(true), t0);
        let effects = s.apply(Event::Click(Hit::Body), t0);
        assert!(!effects.iter().any(|e| matches!(e, Effect::OpenDrive(_))));
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Idle);
        // Without a click it goes on its own.
        let mut s = st();
        s.apply(Event::Notice(safe()), t0);
        s.apply(Event::Tick, t0 + NOTICE_DURATION);
        assert_eq!(s.view(), View::Idle);
        // Both queue behind each other like any notice.
        s.apply(Event::Notice(plugged('F')), t0);
        s.apply(Event::Notice(safe()), t0);
        assert_eq!(s.view(), View::Notice(plugged('F')));
        s.apply(Event::Tick, t0 + NOTICE_DURATION);
        assert_eq!(s.view(), View::Notice(safe()));
    }

    fn copied(body: &str) -> Notice {
        Notice {
            key: "clip-copied".into(),
            source: "Clipboard".into(),
            title: "Disalin".into(),
            body: body.into(),
            kind: NoticeKind::Copied,
        }
    }

    #[test]
    fn copied_is_short_and_the_newest_replaces_it() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(copied("a")), t0);
        s.apply(Event::Notice(copied("b")), t0);
        assert_eq!(s.view(), View::Notice(copied("b")));
        assert_eq!(s.next_deadline(), Some(t0 + COPIED_DURATION));
        s.apply(Event::Tick, t0 + COPIED_DURATION);
        assert_eq!(s.view(), View::Idle, "nothing queued behind it");
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(copied("c")), t0 + secs(5));
        assert_eq!(s.view(), View::Hidden);
    }

    #[test]
    fn copied_never_replaces_a_cleaner_or_system_notice() {
        let t0 = Instant::now();
        let mut s = st();
        // A clean finished; a copy right after waits its turn behind it.
        let done = mole_notice(NoticeKind::CleanDone);
        s.apply(Event::Notice(done.clone()), t0);
        s.apply(Event::Notice(copied("a")), t0);
        s.apply(Event::Notice(system_notice()), t0);
        s.apply(Event::Notice(copied("b")), t0);
        assert_eq!(s.view(), View::Notice(done));
        assert_eq!(s.next_deadline(), Some(t0 + NOTICE_DURATION));
        // The system notice keeps its place; only the newer copy is said.
        let t1 = t0 + NOTICE_DURATION;
        s.apply(Event::Tick, t1);
        assert_eq!(s.view(), View::Notice(system_notice()));
        let t2 = t1 + NOTICE_DURATION;
        s.apply(Event::Tick, t2);
        assert_eq!(s.view(), View::Notice(copied("b")));
        assert_eq!(s.next_deadline(), Some(t2 + COPIED_DURATION));
        s.apply(Event::Tick, t2 + COPIED_DURATION);
        assert_eq!(s.view(), View::Idle);
        // A Mole failure is sticky: a copy shows over it briefly, then it is back.
        let failed = mole_notice(NoticeKind::MoleFailed);
        let t3 = t2 + secs(10);
        s.apply(Event::Notice(failed.clone()), t3);
        assert_eq!(s.view(), View::Notice(failed.clone()));
        s.apply(Event::Notice(copied("c")), t3);
        s.apply(Event::Tick, t3 + COPIED_DURATION);
        assert_eq!(s.view(), View::Notice(failed));
    }

    fn clip_preview(id: u64) -> Notice {
        Notice {
            key: format!("clip-preview-{id}"),
            source: "Clipboard".into(),
            title: format!("Salinan {id}"),
            body: "Teks · 9 karakter".into(),
            kind: NoticeKind::ClipPreview { id },
        }
    }

    #[test]
    fn a_copy_preview_lasts_three_seconds_and_the_latest_replaces_older_ones() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(clip_preview(1)), t0);
        assert_eq!(s.view(), View::Notice(clip_preview(1)));
        assert_eq!(CLIP_NOTICE_DURATION, secs(3));
        assert_eq!(s.next_deadline(), Some(t0 + CLIP_NOTICE_DURATION));
        // A newer one replaces the current one, and a queued one.
        s.apply(Event::Notice(clip_preview(2)), t0 + secs(1));
        assert_eq!(s.view(), View::Notice(clip_preview(2)));
        assert_eq!(s.next_deadline(), Some(t0 + secs(1) + CLIP_NOTICE_DURATION));
        let done = mole_notice(NoticeKind::CleanDone);
        s.apply(Event::Notice(done.clone()), t0 + secs(1));
        s.apply(Event::Notice(clip_preview(3)), t0 + secs(1));
        s.apply(Event::Notice(clip_preview(4)), t0 + secs(1));
        s.apply(Event::Tick, t0 + secs(1) + CLIP_NOTICE_DURATION);
        assert_eq!(
            s.view(),
            View::Notice(done),
            "the clean notice keeps its turn"
        );
        let t1 = t0 + secs(1) + CLIP_NOTICE_DURATION + NOTICE_DURATION;
        s.apply(Event::Tick, t1);
        assert_eq!(s.view(), View::Notice(clip_preview(4)), "only the latest");
        s.apply(Event::Tick, t1 + CLIP_NOTICE_DURATION);
        assert_eq!(s.view(), View::Idle, "nothing queued behind it");
    }

    #[test]
    fn a_copy_preview_replaces_a_copied_notice_and_a_screenshot_replaces_it() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(copied("a")), t0);
        s.apply(Event::Notice(clip_preview(1)), t0);
        assert_eq!(s.view(), View::Notice(clip_preview(1)));
        s.apply(Event::Tick, t0 + CLIP_NOTICE_DURATION);
        assert_eq!(s.view(), View::Idle, "the Disalin did not wait behind it");
        // Queued behind another notice: still replaced, as Copied is.
        let done = mole_notice(NoticeKind::CleanDone);
        s.apply(Event::Notice(done.clone()), t0);
        s.apply(Event::Notice(copied("b")), t0);
        s.apply(Event::Notice(clip_preview(2)), t0);
        s.apply(Event::Notice(shot(1, false)), t0);
        s.apply(Event::Tick, t0 + NOTICE_DURATION);
        assert_eq!(s.view(), View::Notice(shot(1, false)));
        // The screenshot took the current preview's place too.
        let mut s = st();
        s.apply(Event::Notice(clip_preview(3)), t0);
        s.apply(Event::Notice(shot(2, false)), t0);
        assert_eq!(s.view(), View::Notice(shot(2, false)));
        s.apply(Event::Tick, t0 + secs(8));
        assert_eq!(s.view(), View::Idle, "the preview did not come back");
    }

    #[test]
    fn a_copy_preview_never_shows_over_a_fullscreen_app() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(clip_preview(1)), t0);
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Fullscreen(false), t0 + secs(1));
        assert_eq!(s.view(), View::Idle, "dropped, not queued for afterwards");
        // One already up or queued goes when a fullscreen app starts.
        s.apply(Event::Notice(clip_preview(2)), t0 + secs(2));
        s.apply(Event::Fullscreen(true), t0 + secs(2));
        s.apply(Event::Fullscreen(false), t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
        let done = mole_notice(NoticeKind::CleanDone);
        s.apply(Event::Notice(done.clone()), t0 + secs(3));
        s.apply(Event::Notice(clip_preview(3)), t0 + secs(3));
        s.apply(Event::Fullscreen(true), t0 + secs(3));
        s.apply(Event::Fullscreen(false), t0 + secs(3));
        s.apply(Event::Tick, t0 + secs(3) + NOTICE_DURATION);
        assert_eq!(s.view(), View::Idle, "the queued preview was dropped");
    }

    #[test]
    fn a_copy_preview_has_a_button_so_hover_waits_for_it() {
        let t0 = Instant::now();
        let mut s = st();
        assert!(notice_has_action(&clip_preview(1)));
        assert!(!notice_has_action(&copied("a")), "Disalin has none");
        s.apply(Event::Notice(clip_preview(1)), t0);
        // The pointer's visit began on it: no panel over it, and it stays
        // while the pointer does.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(clip_preview(1)));
        s.apply(Event::Tick, t0 + CLIP_NOTICE_DURATION + secs(5));
        assert_eq!(
            s.view(),
            View::Notice(clip_preview(1)),
            "held under the pointer"
        );
        // Once it leaves, the grace first.
        let left = t0 + secs(10);
        s.apply(Event::Hover(false), left);
        s.apply(Event::Tick, left + secs(1));
        assert_eq!(s.view(), View::Notice(clip_preview(1)));
        s.apply(Event::Tick, left + SHOT_LEAVE_GRACE);
        assert_eq!(s.view(), View::Idle);
        // A notice that arrives under a pointer already there runs its
        // normal time.
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Hover(true), t0 + secs(1));
        s.apply(Event::Notice(clip_preview(2)), t0 + secs(2));
        s.apply(Event::Tick, t0 + secs(2) + CLIP_NOTICE_DURATION);
        assert!(!matches!(s.view(), View::Notice(_)));
    }

    #[test]
    fn a_copy_preview_click_opens_the_clipboard_page() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(clip_preview(1)), t0);
        s.apply(Event::Hover(true), t0);
        assert!(!s.panel_is_open());
        assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![]);
        assert_eq!(s.view(), View::Panel(Page::Clipboard));
        // Hover mode: the pointer holds it, nothing is latched.
        assert!(!s.outside_click_wanted());
        s.apply(Event::Hover(false), t0 + secs(1));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn a_copy_preview_click_keeps_the_panel_in_the_staying_modes() {
        let t0 = Instant::now();
        for m in [OpenMode::HoverStay, OpenMode::Click] {
            let mut s = mode(m, t0);
            s.apply(Event::Notice(clip_preview(1)), t0);
            s.apply(Event::Hover(true), t0);
            s.apply(Event::Click(Hit::Body), t0);
            assert_eq!(s.view(), View::Panel(Page::Clipboard), "{m:?}");
            s.apply(Event::Hover(false), t0 + secs(1));
            assert_eq!(s.view(), View::Panel(Page::Clipboard), "{m:?}: it stays");
            assert!(s.outside_click_wanted(), "{m:?}");
        }
    }

    #[test]
    fn a_copy_preview_click_only_dismisses_with_the_clipboard_page_off() {
        let t0 = Instant::now();
        for m in [OpenMode::Hover, OpenMode::HoverStay, OpenMode::Click] {
            let mut s = mode(m, t0);
            let mut pages = PanelPages::default();
            pages.toggle(Page::Clipboard);
            s.apply(Event::PanelPages(pages), t0);
            s.apply(Event::Notice(clip_preview(1)), t0);
            s.apply(Event::Hover(true), t0);
            assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![], "{m:?}");
            assert_eq!(s.view(), View::Idle, "{m:?}: dismissed, no panel");
            assert!(!s.outside_click_wanted(), "{m:?}");
            assert_ne!(s.page(), Page::Clipboard, "{m:?}");
        }
    }

    #[test]
    fn shortcut_collapses_the_panel_until_the_pointer_leaves() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Shortcuts, t0);
        let effects = s.apply(Event::Click(Hit::Shortcut(Shortcut::Emoji)), t0);
        assert_eq!(effects, vec![Effect::RunShortcut(Shortcut::Emoji)]);
        assert_eq!(
            s.view(),
            View::Idle,
            "collapsed with the pointer still over it"
        );
        // Still hovering: nothing reopens it.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Idle);
        // A new visit opens it again, on the same page.
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
    }

    #[test]
    fn shortcut_collapse_beats_an_open_editor() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Shortcuts, t0);
        s.apply(Event::EditorOpen(true), t0);
        s.apply(Event::Click(Hit::Shortcut(Shortcut::Screenshot)), t0);
        assert_eq!(s.view(), View::Idle);
        // The shrunk panel leaves the pointer over transparent pixels: the
        // leave comes at once and must not reopen it for the editor while
        // the screen clip starts.
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Idle, "after the leave");
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Idle, "later");
        // The user comes back: the panel opens, and the editor holds it open
        // again once the pointer leaves.
        s.apply(Event::Hover(true), t0 + secs(6));
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
        s.apply(Event::Hover(false), t0 + secs(7));
        assert_eq!(s.view(), View::Panel(Page::Shortcuts), "pinned");
    }

    #[test]
    fn panel_pinned_by_an_editor_closes_for_a_shortcut_without_hover() {
        // Hovering ended before the click lands (the leave raced the click):
        // the shortcut still shuts the pinned panel until the next visit.
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Shortcuts, t0);
        s.apply(Event::EditorOpen(true), t0);
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
        s.apply(Event::Click(Hit::Shortcut(Shortcut::Emoji)), t0);
        assert_eq!(s.view(), View::Idle);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
    }

    #[test]
    fn shortcut_click_outside_the_panel_does_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(
            s.apply(Event::Click(Hit::Shortcut(Shortcut::Lock)), t0),
            vec![]
        );
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn clicking_the_terminal_asks_for_the_keyboard_and_focus_locks_the_panel() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::CliInput), t0),
            vec![Effect::FocusCliInput]
        );
        s.apply(Event::CliFocus(true), t0);
        // The pointer leaves: the panel stays while the input is used.
        assert_eq!(s.apply(Event::Hover(false), t0), vec![]);
        assert_eq!(s.view(), View::Panel(Page::Cli));
        s.apply(Event::Tick, t0 + secs(30));
        assert_eq!(s.view(), View::Panel(Page::Cli));
        // Esc or a click elsewhere: the island lets go, the panel closes.
        assert_eq!(s.apply(Event::CliFocus(false), t0), vec![]);
        assert_eq!(s.view(), View::Idle);
        // Output arriving changes nothing in the reducer.
        let output = crate::cli::CliEvent::Output {
            run: 1,
            lines: vec!["x".into()],
        };
        assert_eq!(s.apply(Event::Cli(output), t0), vec![]);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn cli_buttons_act_only_on_the_cli_page() {
        let t0 = Instant::now();
        let mut s = st();
        for hit in [Hit::CliInput, Hit::CliStop, Hit::CliExpand] {
            assert_eq!(s.apply(Event::Click(hit.clone()), t0), vec![], "{hit:?}");
        }
        open_on(&mut s, Page::Notes, t0);
        for hit in [Hit::CliInput, Hit::CliStop, Hit::CliExpand] {
            assert_eq!(s.apply(Event::Click(hit.clone()), t0), vec![], "{hit:?}");
        }
        assert!(!s.cli_tall());
        open_on(&mut s, Page::Cli, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::CliStop), t0),
            vec![Effect::StopCli]
        );
    }

    #[test]
    fn leaving_the_cli_page_releases_the_keyboard() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::CliFocus(true), t0);
        // The wheel over anything but the output still pages.
        let effects = s.apply(Event::Wheel(-120), t0);
        assert_eq!(effects, vec![Effect::ReleaseCliFocus]);
        assert!(!s.cli_focused());
        assert_eq!(s.view(), View::Panel(Page::Arrange));
    }

    #[test]
    fn a_shortcut_collapse_releases_the_keyboard() {
        // The input only has the keyboard on the CLI page, so a collapse
        // needs a stale focus flag; the reducer still lets go at once.
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Shortcuts, t0);
        s.cli_focused = true;
        let effects = s.apply(Event::Click(Hit::Shortcut(Shortcut::Emoji)), t0);
        assert_eq!(
            effects,
            vec![
                Effect::RunShortcut(Shortcut::Emoji),
                Effect::ReleaseCliFocus
            ]
        );
        assert_eq!(s.view(), View::Idle);
        assert!(!s.cli_focused());
    }

    fn pick(path: &str) -> Hit {
        Hit::CliFolderPick {
            path: path.into(),
            favorite: false,
        }
    }

    #[test]
    fn the_folder_chip_toggles_the_menu_without_touching_the_keyboard() {
        let t0 = Instant::now();
        let mut s = st();
        // Off the CLI page the chip does nothing.
        open_on(&mut s, Page::Notes, t0);
        assert_eq!(s.apply(Event::Click(Hit::CliFolder), t0), vec![]);
        assert!(!s.cli_menu());
        open_on(&mut s, Page::Cli, t0);
        assert_eq!(s.apply(Event::Click(Hit::CliFolder), t0), vec![]);
        assert!(s.cli_menu());
        assert_eq!(s.apply(Event::Click(Hit::CliFolder), t0), vec![]);
        assert!(!s.cli_menu(), "the chip again closes it");
        // While the input has the keyboard, the menu leaves it alone.
        s.apply(Event::CliFocus(true), t0);
        assert_eq!(s.apply(Event::Click(Hit::CliFolder), t0), vec![]);
        assert!(s.cli_menu() && s.cli_focused());
        // Starring keeps the menu open.
        let star = Hit::CliFolderStar {
            path: r"D:\a".into(),
            on: true,
        };
        assert_eq!(
            s.apply(Event::Click(star), t0),
            vec![Effect::CliFavorite {
                path: r"D:\a".into(),
                on: true
            }]
        );
        assert!(s.cli_menu());
        // A pick closes it and moves the CLI; focus stays as it was.
        assert_eq!(
            s.apply(Event::Click(pick(r"D:\b")), t0),
            vec![Effect::CliChangeFolder(r"D:\b".into())]
        );
        assert!(!s.cli_menu() && s.cli_focused());
        // A pick without the menu (a stale click) does nothing.
        assert_eq!(s.apply(Event::Click(pick(r"D:\b")), t0), vec![]);
        // The backdrop closes it, as Esc does (`App` sends the same click).
        s.apply(Event::Click(Hit::CliFolder), t0);
        assert_eq!(s.apply(Event::Click(Hit::CliMenuDismiss), t0), vec![]);
        assert!(!s.cli_menu() && s.cli_focused());
    }

    #[test]
    fn the_folder_menu_closes_with_its_page_or_the_panel() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::Click(Hit::CliFolder), t0);
        assert!(s.cli_menu());
        // A page change.
        s.apply(Event::Wheel(-120), t0);
        assert!(!s.cli_menu());
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::Click(Hit::CliFolder), t0);
        // The panel closing; it reopens without the menu.
        assert_eq!(leave(&mut s, t0), vec![]);
        assert!(!s.cli_menu());
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Cli));
        assert!(!s.cli_menu());
        // Explorer results reach `App` only.
        let read = Event::ExplorerFolders {
            request: 1,
            folders: vec![r"D:\x".into()],
        };
        assert_eq!(s.apply(read, t0), vec![]);
    }

    #[test]
    fn cli_output_scrolls_and_clamps() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        let scroll = |s: &mut IslandState, lines| {
            s.apply(Event::CliScroll { lines, max: 30 }, t0);
            s.cli_scroll()
        };
        assert_eq!(scroll(&mut s, 3), 3, "wheel up over the output");
        assert_eq!(s.view(), View::Panel(Page::Cli), "no page change");
        assert_eq!(scroll(&mut s, -1), 2);
        assert_eq!(scroll(&mut s, -10), 0, "never below the newest");
        // PageUp / PageDown move a page of lines.
        assert_eq!(scroll(&mut s, 8), 8);
        assert_eq!(scroll(&mut s, 100), 30, "never past the oldest");
        assert_eq!(scroll(&mut s, -8), 22);
        // New output keeps a scrolled view on its lines...
        s.apply(Event::CliOutput(4), t0);
        assert_eq!(s.cli_scroll(), 26);
        // ...and one at the bottom on the newest.
        scroll(&mut s, -100);
        s.apply(Event::CliOutput(4), t0);
        assert_eq!(s.cli_scroll(), 0);
        // Closing the panel starts it at the bottom again.
        scroll(&mut s, 5);
        leave(&mut s, t0);
        assert_eq!(s.cli_scroll(), 0);
        // Scrolling does nothing off the CLI page.
        open_on(&mut s, Page::Notes, t0);
        assert_eq!(scroll(&mut s, 5), 0);
    }

    #[test]
    fn wheel_elsewhere_still_pages_on_the_cli_page() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::Wheel(120), t0);
        assert_eq!(s.view(), View::Panel(Page::Devices));
    }

    #[test]
    fn the_enlarged_panel_goes_back_to_small_when_it_closes() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::Click(Hit::CliExpand), t0);
        assert!(s.cli_tall());
        // Another page and back: still enlarged for the CLI.
        s.apply(Event::Wheel(-120), t0);
        s.apply(Event::Wheel(120), t0);
        assert!(s.cli_tall());
        s.apply(Event::Click(Hit::CliExpand), t0);
        assert!(!s.cli_tall(), "the button shrinks it again");
        s.apply(Event::Click(Hit::CliExpand), t0);
        // Focused, the pointer leaving keeps it; letting go closes it.
        s.apply(Event::CliFocus(true), t0);
        leave(&mut s, t0);
        assert!(s.cli_tall());
        s.apply(Event::CliFocus(false), t0);
        assert!(!s.cli_tall());
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Cli));
        assert!(!s.cli_tall());
    }

    #[test]
    fn wheel_ignored_when_panel_closed() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Wheel(-120), t0);
        s.apply(Event::Wheel(-60), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Wheel(-60), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn wheel_leftover_resets_when_panel_closes() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Wheel(-90), t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Wheel(-90), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn temporary_notice_blocks_panel_then_opens_if_still_hovered() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(plain_notice("k1")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Notice(plain_notice("k1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn waiting_does_not_block_panel_and_shows_strip() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        assert_eq!(s.waiting_strip(), None);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert_eq!(s.waiting_strip(), Some(&waiting("s1")));
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Notice(waiting("s1")));
        assert_eq!(s.waiting_strip(), None);
    }

    #[test]
    fn click_inside_panel_keeps_waiting() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![]);
        assert_eq!(s.waiting_strip(), Some(&waiting("s1")));
    }

    #[test]
    fn clicking_strip_dismisses_waiting() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(waiting("s2")), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.waiting_strip(), Some(&waiting("s2")));
        assert_eq!(s.apply(Event::Click(Hit::Strip), t0), vec![]);
        assert_eq!(s.view(), View::Panel(Page::Calendar), "panel stays open");
        assert_eq!(
            s.waiting_strip(),
            Some(&waiting("s1")),
            "older one shows next"
        );
        s.apply(Event::Click(Hit::Strip), t0);
        assert_eq!(s.waiting_strip(), None);
        leave(&mut s, t0);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn editor_keeps_panel_open() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::EditorOpen(true), t0);
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::EditorOpen(false), t0);
        // Closing it keeps the panel a moment (`ACTIVITY_LINGER`), no more.
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Tick, t0 + ACTIVITY_LINGER);
        assert_eq!(s.view(), View::Idle);
    }

    // ---- the panel lingers after the user did something in it ----

    /// Hovers, opens an editor, leaves and closes it: the panel is lingering.
    fn lingering_after_editor(t0: Instant) -> IslandState {
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::EditorOpen(true), t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::EditorOpen(false), t0 + secs(1));
        s
    }

    #[test]
    fn an_editor_closing_with_the_pointer_away_keeps_the_panel_four_seconds() {
        let t0 = Instant::now();
        let mut s = lingering_after_editor(t0);
        assert_eq!(ACTIVITY_LINGER, secs(4));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        // The deadline is on `next_deadline`, so the existing timer ends it.
        assert_eq!(s.next_deadline(), Some(t0 + secs(5)));
        s.apply(Event::Tick, t0 + secs(4));
        assert_eq!(s.view(), View::Panel(Page::Calendar), "1 s of 4 left");
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn a_click_in_the_panel_arms_or_refreshes_the_linger() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.next_deadline(), None, "hover alone arms nothing");
        s.apply(Event::Click(Hit::NotesMore), t0 + secs(1));
        assert_eq!(s.next_deadline(), Some(t0 + secs(5)));
        // Leaving keeps what is left; a new click re-arms it.
        s.apply(Event::Hover(false), t0 + secs(2));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Hover(true), t0 + secs(3));
        s.apply(Event::Click(Hit::NotesMore), t0 + secs(3));
        s.apply(Event::Hover(false), t0 + secs(3));
        assert_eq!(s.next_deadline(), Some(t0 + secs(7)));
        s.apply(Event::Tick, t0 + secs(6));
        assert!(s.panel_is_open());
        s.apply(Event::Tick, t0 + secs(7));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn a_wheel_page_turn_lingers_but_a_spent_notch_does_not() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Wheel(-60), t0);
        assert_eq!(s.next_deadline(), None, "half a notch turned no page");
        s.apply(Event::Wheel(-60), t0 + secs(1));
        assert_eq!(s.page(), Page::Pomodoro);
        assert_eq!(s.next_deadline(), Some(t0 + secs(5)));
        s.apply(Event::Hover(false), t0 + secs(2));
        assert!(s.panel_is_open());
    }

    #[test]
    fn losing_the_keyboard_lingers_only_while_something_still_holds_the_panel() {
        let t0 = Instant::now();
        // Held by the pointer too: leaving keeps the remainder.
        let mut s = st();
        open_on(&mut s, Page::Clipboard, t0);
        s.apply(Event::ClipFocus(true), t0);
        s.apply(Event::ClipFocus(false), t0 + secs(2));
        assert_eq!(s.next_deadline(), Some(t0 + secs(6)));
        // The input alone held it: letting go closes it, as Esc always did.
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::CliFocus(true), t0);
        leave(&mut s, t0);
        s.apply(Event::CliFocus(false), t0);
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
        // The click that focused the input armed a linger, then the pointer
        // left: Esc still closes it, it does not re-arm a fresh 4 s.
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::Click(Hit::CliInput), t0);
        s.apply(Event::CliFocus(true), t0);
        s.apply(Event::Hover(false), t0 + secs(1));
        assert!(s.panel_is_open());
        s.apply(Event::CliFocus(false), t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn the_hotkey_closes_a_lingering_clipboard_page() {
        let t0 = Instant::now();
        // The wheel to the page armed the linger; the pointer left.
        let mut s = st();
        open_on(&mut s, Page::Clipboard, t0);
        s.apply(Event::Hover(false), t0 + secs(1));
        assert_eq!(s.view(), View::Panel(Page::Clipboard));
        s.apply(Event::ClipHotkey, t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
        // The next visit opens it again.
        s.apply(Event::Hover(true), t0 + secs(3));
        assert!(s.panel_is_open());
    }

    #[test]
    fn a_click_outside_ends_the_linger() {
        let t0 = Instant::now();
        let mut s = lingering_after_editor(t0);
        assert!(
            s.outside_click_wanted(),
            "the poll watches while it lingers"
        );
        s.apply(Event::Dismiss, t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
        assert!(!s.outside_click_wanted());
    }

    #[test]
    fn a_collapse_beats_the_linger() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Click(Hit::NotesMore), t0);
        assert!(s.next_deadline().is_some());
        // A tile: the panel shuts for it, linger and all, under the pointer.
        let effects = s.apply(Event::Click(Hit::SystemProcess), t0 + secs(1));
        assert_eq!(effects, vec![Effect::RunShortcut(Shortcut::TaskManager)]);
        assert!(!s.panel_is_open());
        assert_eq!(s.next_deadline(), None);
        // Away and back: the new visit opens it, nothing lingers.
        s.apply(Event::Hover(false), t0 + secs(2));
        assert!(!s.panel_is_open());
        s.apply(Event::Hover(true), t0 + secs(3));
        s.apply(Event::Hover(false), t0 + secs(3));
        assert_eq!(s.view(), View::Idle);
        // A copy with Enter, and the search closing, do the same.
        let mut s = st();
        open_on(&mut s, Page::Clipboard, t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::ClipCommit, t0);
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn fullscreen_a_mode_change_or_the_panel_closing_end_it() {
        let t0 = Instant::now();
        let mut s = lingering_after_editor(t0);
        s.apply(Event::Fullscreen(true), t0 + secs(2));
        assert_eq!(s.next_deadline(), None);
        s.apply(Event::Fullscreen(false), t0 + secs(3));
        assert_eq!(s.view(), View::Idle, "no panel comes back from a game");
        let mut s = lingering_after_editor(t0);
        s.apply(Event::OpenMode(OpenMode::Click), t0 + secs(2));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn a_closed_panel_is_never_reopened_by_what_arms_the_linger() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::EditorOpen(true), t0);
        // Pointer never came: the editor held the panel and was closed.
        s.apply(Event::EditorOpen(false), t0 + secs(1));
        assert_eq!(s.view(), View::Panel(Page::Calendar), "it did hold it");
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Idle);
        // Now closed: the same events open nothing.
        s.apply(Event::EditorOpen(false), t0 + secs(6));
        s.apply(Event::Click(Hit::NotesMore), t0 + secs(6));
        s.apply(Event::Wheel(-120), t0 + secs(6));
        s.apply(Event::CliFocus(false), t0 + secs(6));
        s.apply(Event::ClipFocus(false), t0 + secs(6));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
        // A late event after the linger ran out but before its Tick does not
        // re-arm it either.
        let mut s = lingering_after_editor(t0);
        s.apply(Event::EditorOpen(false), t0 + secs(10));
        assert_eq!(s.view(), View::Idle);
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn hover_holds_the_panel_through_the_linger() {
        let t0 = Instant::now();
        let mut s = lingering_after_editor(t0);
        s.apply(Event::Hover(true), t0 + secs(2));
        s.apply(Event::Tick, t0 + secs(6));
        assert_eq!(
            s.view(),
            View::Panel(Page::Calendar),
            "the pointer holds it"
        );
        assert_eq!(s.next_deadline(), None);
        s.apply(Event::Hover(false), t0 + secs(7));
        assert_eq!(s.view(), View::Idle, "nothing of the linger is left");
    }

    #[test]
    fn a_guarded_media_click_arms_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        open_on(&mut s, Page::Media, t0);
        s.apply(Event::Dismiss, t0);
        let early = t0 + Duration::from_millis(100);
        assert!(s.media_guarded(early));
        assert_eq!(s.apply(Event::Click(Hit::PlayPause), early), vec![]);
        assert_eq!(s.next_deadline(), None);
    }

    // ---- v2: pomodoro ----

    #[test]
    fn priority_pomodoro_over_media() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        assert_eq!(s.view(), View::PomodoroCompact);
        s.apply(Event::Click(Hit::PomoStartPause), t0 + secs(10));
        assert!(matches!(s.pomodoro().status(), PomoStatus::Paused { .. }));
        assert_eq!(s.view(), View::PomodoroCompact, "paused still counts");
        s.apply(Event::Click(Hit::PomoReset), t0 + secs(20));
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn waiting_outranks_pomodoro() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        s.apply(Event::Notice(waiting("s1")), t0);
        assert_eq!(s.view(), View::Notice(waiting("s1")));
    }

    #[test]
    fn phase_done_queues_notice_and_sound() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        assert_eq!(s.next_deadline(), Some(t0 + mins(25)));
        let effects = s.apply(Event::Tick, t0 + mins(25));
        assert!(effects.contains(&Effect::PlaySound), "{effects:?}");
        let view = s.view();
        assert_eq!(
            notice_title(&view),
            Some((
                &NoticeKind::Pomodoro,
                "Fokus selesai",
                "Istirahat 5 menit · klik untuk mulai"
            ))
        );
        let View::Notice(n) = &view else { panic!() };
        assert_eq!(
            (n.source.as_str(), n.key.as_str()),
            ("Pomodoro", "pomodoro")
        );
        s.apply(Event::Tick, t0 + mins(25) + secs(5));
        assert_eq!(s.view(), View::PomodoroCompact);
        assert_eq!(s.pomodoro().status(), PomoStatus::Ready);
    }

    #[test]
    fn break_done_notice_names_next_focus_session() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoSkip), t0);
        let effects = s.apply(Event::Click(Hit::PomoSkip), t0 + secs(1));
        assert_eq!(effects, vec![Effect::PlaySound]);
        assert_eq!(
            notice_title(&s.view()),
            Some((
                &NoticeKind::Pomodoro,
                "Istirahat selesai",
                "Fokus sesi 2 dari 4 · klik untuk mulai"
            ))
        );
    }

    #[test]
    fn last_focus_announces_long_break() {
        let t0 = Instant::now();
        let mut s = IslandState::new(
            date(2026, 10, 25),
            PomoSettings {
                sessions: 1,
                ..PomoSettings::default()
            },
        );
        s.apply(Event::Click(Hit::PomoSkip), t0);
        assert_eq!(
            notice_title(&s.view()).map(|t| t.2),
            Some("Istirahat 15 menit · klik untuk mulai")
        );
    }

    #[test]
    fn session_label_never_exceeds_session_count() {
        let t0 = Instant::now();
        let mut s = st();
        // Focus 1 → short break → focus 2 → short break, then lower sessions to 1.
        s.apply(Event::Click(Hit::PomoSkip), t0);
        s.apply(Event::Click(Hit::PomoSkip), t0);
        s.apply(Event::Click(Hit::PomoSkip), t0);
        for _ in 0..3 {
            s.apply(Event::Click(Hit::PomoAdjust(PomoField::Sessions, -1)), t0);
        }
        assert_eq!(s.pomodoro().settings().sessions, 1);
        s.apply(Event::Click(Hit::PomoSkip), t0);
        assert_eq!(
            notice_title(&s.view()).map(|t| t.2),
            Some("Fokus sesi 1 dari 1 · klik untuk mulai")
        );
    }

    #[test]
    fn pomodoro_notices_follow_the_language() {
        i18n::set_lang(i18n::Lang::En);
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoSkip), t0);
        assert_eq!(
            notice_title(&s.view()),
            Some((
                &NoticeKind::Pomodoro,
                "Focus done",
                "5-minute break · click to start"
            ))
        );
        s.apply(Event::Click(Hit::PomoSkip), t0 + secs(1));
        assert_eq!(
            notice_title(&s.view()).map(|t| (t.1, t.2)),
            Some(("Break done", "Focus session 2 of 4 · click to start"))
        );
    }

    #[test]
    fn repeated_skips_replace_the_pomodoro_notice() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoSkip), t0);
        s.apply(Event::Click(Hit::PomoSkip), t0 + secs(1));
        assert_eq!(
            notice_title(&s.view()).map(|t| t.1),
            Some("Istirahat selesai")
        );
        s.apply(Event::Tick, t0 + secs(6));
        assert_eq!(s.view(), View::PomodoroCompact);
    }

    #[test]
    fn click_ready_compact_starts_next_phase() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        s.apply(Event::Tick, t0 + mins(25));
        s.apply(Event::Tick, t0 + mins(25) + secs(5));
        assert_eq!(s.view(), View::PomodoroCompact);
        let at = t0 + mins(26);
        assert_eq!(s.apply(Event::Click(Hit::Body), at), vec![]);
        assert_eq!(s.pomodoro().phase(), Phase::ShortBreak);
        assert_eq!(
            s.pomodoro().status(),
            PomoStatus::Running {
                ends_at: at + mins(5)
            }
        );
    }

    #[test]
    fn click_running_compact_does_not_pause() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        s.apply(Event::Click(Hit::Body), t0 + secs(30));
        assert!(matches!(s.pomodoro().status(), PomoStatus::Running { .. }));
    }

    #[test]
    fn click_pomodoro_notice_starts_next_phase() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        s.apply(Event::Tick, t0 + mins(25));
        let at = t0 + mins(25) + secs(2);
        assert_eq!(s.apply(Event::Click(Hit::Body), at), vec![]);
        assert_eq!(
            s.pomodoro().status(),
            PomoStatus::Running {
                ends_at: at + mins(5)
            }
        );
        assert_eq!(s.view(), View::PomodoroCompact);
    }

    #[test]
    fn input_after_deadline_finishes_phase_first() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        // The deadline passed but its Tick has not arrived yet.
        let effects = s.apply(Event::Click(Hit::PomoStartPause), t0 + mins(25) + secs(1));
        assert!(effects.contains(&Effect::PlaySound));
        assert_ne!(
            s.pomodoro().status(),
            PomoStatus::Paused {
                remaining: Duration::ZERO
            }
        );
        assert_eq!(s.pomodoro().phase(), Phase::ShortBreak);
    }

    #[test]
    fn next_deadline_includes_pomodoro_end() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        s.apply(Event::Notice(app_notice("k1")), t0 + secs(1));
        assert_eq!(s.next_deadline(), Some(t0 + secs(6)));
        s.apply(Event::Tick, t0 + secs(6));
        assert_eq!(s.next_deadline(), Some(t0 + mins(25)));
    }

    #[test]
    fn adjust_emits_save() {
        let t0 = Instant::now();
        let mut s = st();
        let effects = s.apply(Event::Click(Hit::PomoAdjust(PomoField::Focus, 1)), t0);
        assert_eq!(
            effects,
            vec![Effect::SavePomodoro(PomoSettings {
                focus_min: 30,
                ..PomoSettings::default()
            })]
        );
    }

    #[test]
    fn adjust_without_change_saves_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::PomoStartPause), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::PomoAdjust(PomoField::Focus, 1)), t0),
            vec![]
        );
    }

    #[test]
    fn save_sends_clamped_settings() {
        let t0 = Instant::now();
        let mut s = IslandState::new(
            date(2026, 10, 25),
            PomoSettings {
                focus_min: 0,
                ..PomoSettings::default()
            },
        );
        assert_eq!(
            s.apply(Event::Click(Hit::PomoAdjust(PomoField::Sessions, 1)), t0),
            vec![Effect::SavePomodoro(PomoSettings {
                focus_min: 5,
                sessions: 5,
                ..PomoSettings::default()
            })]
        );
    }

    // ---- v2: calendar ----

    fn refresh(from: NaiveDate, to: NaiveDate) -> Effect {
        Effect::RefreshCalendar { from, to }
    }

    #[test]
    fn calendar_starts_on_today_month() {
        let s = st();
        assert_eq!(s.cal_view(), CalView::Month);
        assert_eq!(s.cal_anchor(), date(2026, 10, 25));
        assert_eq!(s.cal_selected(), date(2026, 10, 25));
        // Oct 1 2026 is a Thursday.
        assert_eq!(s.cal_range(), (date(2026, 9, 28), date(2026, 11, 9)));
    }

    #[test]
    fn calendar_nav_moves_anchor_and_requests_range() {
        let t0 = Instant::now();
        let mut s = st();
        let effects = s.apply(Event::Click(Hit::CalNext), t0);
        assert_eq!(s.cal_anchor(), date(2026, 11, 25));
        // Nov 1 2026 is a Sunday.
        let from = date(2026, 10, 26);
        assert_eq!(effects, vec![refresh(from, from + chrono::Days::new(42))]);
        s.apply(Event::Click(Hit::CalPrev), t0);
        s.apply(Event::Click(Hit::CalPrev), t0);
        assert_eq!(s.cal_anchor(), date(2026, 9, 25));
    }

    #[test]
    fn month_nav_clamps_to_month_end() {
        let t0 = Instant::now();
        let mut s = IslandState::new(date(2026, 1, 31), PomoSettings::default());
        s.apply(Event::Click(Hit::CalNext), t0);
        assert_eq!(s.cal_anchor(), date(2026, 2, 28));
    }

    #[test]
    fn week_and_day_nav_and_ranges() {
        let t0 = Instant::now();
        let mut s = st();
        let effects = s.apply(Event::Click(Hit::CalViewSel(CalView::Week)), t0);
        // Oct 25 2026 is a Sunday: its week starts Monday Oct 19.
        assert_eq!(
            effects,
            vec![refresh(date(2026, 10, 19), date(2026, 10, 26))]
        );
        s.apply(Event::Click(Hit::CalNext), t0);
        assert_eq!(s.cal_range(), (date(2026, 10, 26), date(2026, 11, 2)));
        s.apply(Event::Click(Hit::CalViewSel(CalView::Day)), t0);
        assert_eq!(
            s.cal_anchor(),
            date(2026, 10, 25),
            "day view opens on selected"
        );
        let effects = s.apply(Event::Click(Hit::CalPrev), t0);
        assert_eq!(
            effects,
            vec![refresh(date(2026, 10, 24), date(2026, 10, 25))]
        );
        assert_eq!(s.cal_selected(), date(2026, 10, 24));
    }

    #[test]
    fn same_view_selection_requests_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(
            s.apply(Event::Click(Hit::CalViewSel(CalView::Month)), t0),
            vec![]
        );
    }

    #[test]
    fn cal_date_selects_without_moving_month() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(
            s.apply(Event::Click(Hit::CalDate(date(2026, 11, 3))), t0),
            vec![]
        );
        assert_eq!(s.cal_selected(), date(2026, 11, 3));
        assert_eq!(s.cal_anchor(), date(2026, 10, 25));
        s.apply(Event::Click(Hit::CalViewSel(CalView::Day)), t0);
        assert_eq!(s.cal_anchor(), date(2026, 11, 3));
        s.apply(Event::Click(Hit::CalDate(date(2026, 11, 5))), t0);
        assert_eq!(
            s.cal_anchor(),
            date(2026, 11, 5),
            "day view follows selection"
        );
    }

    #[test]
    fn cal_add_opens_editor_on_selected_date() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Click(Hit::CalDate(date(2026, 10, 30))), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::CalAdd), t0),
            vec![Effect::OpenEventEditor {
                event_id: None,
                date: date(2026, 10, 30)
            }]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::CalEvent("ev1".into())), t0),
            vec![Effect::OpenEventEditor {
                event_id: Some("ev1".into()),
                date: date(2026, 10, 30)
            }]
        );
    }

    #[test]
    fn cal_connect_starts_google_login() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(
            s.apply(Event::Click(Hit::CalConnect), t0),
            vec![Effect::ConnectGoogle]
        );
    }

    #[test]
    fn opening_panel_on_calendar_requests_refresh() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(
            s.apply(Event::Hover(true), t0),
            vec![refresh(date(2026, 9, 28), date(2026, 11, 9))]
        );
        // Already showing: no repeat.
        assert_eq!(s.apply(Event::Tick, t0), vec![]);
        s.apply(Event::Wheel(-120), t0);
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.apply(Event::Hover(true), t0), vec![], "opens on Pomodoro");
        assert_eq!(
            s.apply(Event::Wheel(120), t0),
            vec![refresh(date(2026, 9, 28), date(2026, 11, 9))],
            "scrolling onto the calendar shows it too"
        );
    }

    #[test]
    fn cal_more_switches_to_day_view() {
        let t0 = Instant::now();
        let mut s = st();
        let effects = s.apply(Event::Click(Hit::CalMore(date(2026, 10, 14))), t0);
        assert_eq!(s.cal_view(), CalView::Day);
        assert_eq!(s.cal_anchor(), date(2026, 10, 14));
        assert_eq!(s.cal_selected(), date(2026, 10, 14));
        assert_eq!(
            effects,
            vec![refresh(date(2026, 10, 14), date(2026, 10, 15))]
        );
    }

    #[test]
    fn cal_more_in_day_view_pages_through_that_day() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Calendar, t0);
        let d = date(2026, 10, 14);
        s.apply(Event::Click(Hit::CalMore(d)), t0);
        assert_eq!((s.cal_view(), s.day_page()), (CalView::Day, 0));
        // Already on that day: next chunk, no new fetch.
        assert_eq!(s.apply(Event::Click(Hit::CalMore(d)), t0), vec![]);
        s.apply(Event::Click(Hit::CalMore(d)), t0);
        assert_eq!(s.day_page(), 2);
        assert_eq!(s.cal_anchor(), d);
    }

    #[test]
    fn day_page_resets_on_navigation_view_change_and_close() {
        let t0 = Instant::now();
        let d = date(2026, 10, 25);
        let paged = |s: &mut IslandState| {
            s.apply(Event::Click(Hit::CalViewSel(CalView::Day)), t0);
            s.apply(Event::Click(Hit::CalMore(d)), t0);
            assert_eq!(s.day_page(), 1);
        };
        let mut s = st();
        open_on(&mut s, Page::Calendar, t0);
        paged(&mut s);
        s.apply(Event::Click(Hit::CalNext), t0);
        assert_eq!(s.day_page(), 0, "next day");

        s.apply(Event::Click(Hit::CalPrev), t0);
        paged(&mut s);
        s.apply(Event::Click(Hit::CalViewSel(CalView::Week)), t0);
        assert_eq!(s.day_page(), 0, "view change");

        paged(&mut s);
        s.apply(Event::Click(Hit::CalDate(date(2026, 10, 26))), t0);
        assert_eq!(s.day_page(), 0, "another date");

        s.apply(Event::Click(Hit::CalDate(d)), t0);
        paged(&mut s);
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.day_page(), 0, "page left the calendar");

        s.apply(Event::Wheel(120), t0);
        paged(&mut s);
        leave(&mut s, t0);
        assert_eq!(s.day_page(), 0, "panel closed");
    }

    #[test]
    fn today_rollover_moves_dates_that_were_today() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Today(date(2026, 10, 26)), t0);
        assert_eq!(s.cal_anchor(), date(2026, 10, 26));
        assert_eq!(s.cal_selected(), date(2026, 10, 26));

        let mut s = st();
        s.apply(Event::Click(Hit::CalDate(date(2026, 10, 20))), t0);
        s.apply(Event::Click(Hit::CalNext), t0);
        s.apply(Event::Today(date(2026, 10, 26)), t0);
        assert_eq!(s.cal_anchor(), date(2026, 11, 25));
        assert_eq!(s.cal_selected(), date(2026, 10, 20));
    }

    // ---- v2: notes ----

    #[test]
    fn note_hits_open_editor() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(
            s.apply(Event::Click(Hit::NoteNew), t0),
            vec![Effect::OpenNoteEditor(None)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::NoteOpen("n1".into())), t0),
            vec![Effect::OpenNoteEditor(Some("n1".into()))]
        );
    }

    // ---- v2.2: reminders ----

    fn reminder(id: &str) -> Notice {
        Notice {
            key: format!("reminder-{id}"),
            source: "Pengingat".into(),
            title: format!("Minum obat {id}"),
            body: "08.00".into(),
            kind: NoticeKind::Reminder { id: id.into() },
        }
    }

    #[test]
    fn reminder_is_sticky_and_click_marks_it_done() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        assert_eq!(s.next_deadline(), None, "no expiry");
        s.apply(Event::Tick, t0 + secs(600));
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        // Claude activity has nothing to do with it.
        s.apply(
            Event::ClaudeActivity {
                session_id: "r1".into(),
            },
            t0 + secs(601),
        );
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        assert_eq!(
            s.apply(Event::Click(Hit::Body), t0 + secs(602)),
            vec![Effect::ReminderDone("r1".into())]
        );
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn newest_sticky_notice_shows_first() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        s.apply(Event::Notice(reminder("r2")), t0);
        assert_eq!(s.view(), View::Notice(reminder("r2")));
        assert_eq!(
            s.apply(Event::Click(Hit::Body), t0),
            vec![Effect::ReminderDone("r2".into())]
        );
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Notice(waiting("s1")));
        // A newer waiting notice goes above an older reminder.
        s.apply(Event::Notice(reminder("r3")), t0);
        s.apply(Event::Notice(waiting("s2")), t0);
        assert_eq!(s.view(), View::Notice(waiting("s2")));
        assert_eq!(s.apply(Event::Click(Hit::Body), t0), vec![]);
        assert_eq!(s.view(), View::Notice(reminder("r3")));
    }

    #[test]
    fn same_reminder_again_is_not_shown_twice() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(reminder("r1")), t0);
        s.apply(Event::Notice(reminder("r2")), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Notice(reminder("r2")));
        s.apply(Event::Click(Hit::Body), t0);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn temporary_notice_preempts_reminder_then_it_returns() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(reminder("r1")), t0);
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Notice(reminder("r1")));
    }

    #[test]
    fn reminder_during_fullscreen_shows_briefly_and_returns_after() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        assert_eq!(s.next_deadline(), Some(t0 + NOTICE_DURATION));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Fullscreen(false), t0 + secs(6));
        assert_eq!(s.view(), View::Notice(reminder("r1")));
    }

    #[test]
    fn clicking_the_brief_fullscreen_reminder_marks_it_done() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        assert_eq!(
            s.apply(Event::Click(Hit::Body), t0 + secs(1)),
            vec![Effect::ReminderDone("r1".into())]
        );
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Fullscreen(false), t0 + secs(2));
        assert_eq!(s.view(), View::Idle, "not sticky any more");
    }

    #[test]
    fn cleared_reminder_disappears_everywhere() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(reminder("r1")), t0);
        s.apply(Event::Notice(reminder("r2")), t0);
        s.apply(Event::ReminderCleared("r2".into()), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        s.apply(Event::ReminderCleared("gone".into()), t0);
        assert_eq!(s.view(), View::Notice(reminder("r1")));
        // Also its brief fullscreen copy, showing or queued.
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(app_notice("k1")), t0);
        s.apply(Event::Notice(reminder("r3")), t0);
        s.apply(Event::ReminderCleared("r3".into()), t0);
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Hidden, "the queued copy went too");
        s.apply(Event::ReminderCleared("r1".into()), t0 + secs(5));
        s.apply(Event::Fullscreen(false), t0 + secs(6));
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn strip_shows_newest_sticky_and_click_marks_reminder_done() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(waiting("s1")), t0);
        // The panel is open when the reminder arrives (a reminder that is
        // there before the pointer is, hides the panel: its button is the
        // target).
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Notice(reminder("r1")), t0);
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        assert_eq!(s.waiting_strip(), Some(&reminder("r1")));
        assert_eq!(
            s.apply(Event::Click(Hit::Strip), t0),
            vec![Effect::ReminderDone("r1".into())]
        );
        assert_eq!(s.waiting_strip(), Some(&waiting("s1")));
        assert_eq!(s.apply(Event::Click(Hit::Strip), t0), vec![]);
        assert_eq!(s.waiting_strip(), None);
    }

    #[test]
    fn reminder_hits_open_editor_and_page() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(
            s.apply(Event::Click(Hit::ReminderNew), t0),
            vec![Effect::OpenReminderEditor(None)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::ReminderOpen("r1".into())), t0),
            vec![Effect::OpenReminderEditor(Some("r1".into()))]
        );
        open_on(&mut s, Page::Reminders, t0);
        assert_eq!(s.reminders_page(), 0);
        s.apply(Event::Click(Hit::RemindersMore), t0);
        s.apply(Event::Click(Hit::RemindersMore), t0);
        assert_eq!(s.reminders_page(), 2);
        leave(&mut s, t0);
        assert_eq!(s.reminders_page(), 0);
    }

    #[test]
    fn reminder_page_sits_between_pomodoro_and_notes() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Pomodoro, t0);
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Reminders));
        s.apply(Event::Wheel(-120), t0);
        assert_eq!(s.view(), View::Panel(Page::Notes));
    }

    #[test]
    fn notes_more_advances_and_resets_on_close() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        assert_eq!(s.notes_page(), 0);
        s.apply(Event::Click(Hit::NotesMore), t0);
        s.apply(Event::Click(Hit::NotesMore), t0);
        assert_eq!(s.notes_page(), 2);
        leave(&mut s, t0);
        assert_eq!(s.notes_page(), 0);
    }

    // ---- v2.2: lyrics ----

    fn lyric_ready(s: &mut IslandState, t0: Instant) {
        s.apply(Event::Media(Some(media(true))), t0);
        s.apply(Event::LyricsSetting(true), t0);
        s.apply(Event::LyricsReady(true), t0);
    }

    #[test]
    fn lyric_view_needs_setting_lyrics_and_playback() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        s.apply(Event::LyricsReady(true), t0);
        assert_eq!(s.view(), View::MediaCompact, "setting off");
        s.apply(Event::LyricsSetting(true), t0);
        assert_eq!(s.view(), View::MediaLyric);
        s.apply(Event::LyricsReady(false), t0);
        assert_eq!(s.view(), View::MediaCompact, "no synced lyrics");
        s.apply(Event::LyricsReady(true), t0);
        s.apply(Event::Media(Some(media(false))), t0);
        assert_eq!(s.view(), View::MediaCompact, "paused shows the title");
        s.apply(Event::Media(Some(media(true))), t0);
        assert_eq!(s.view(), View::MediaLyric, "same track resumes its lyrics");
        s.apply(Event::Media(None), t0);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn lyric_view_ranks_like_the_compact_media() {
        let t0 = Instant::now();
        let mut s = st();
        lyric_ready(&mut s, t0);
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::MediaLyric);
        s.apply(Event::Click(Hit::PomoStartPause), t0 + secs(5));
        assert_eq!(s.view(), View::PomodoroCompact);
        s.apply(Event::Click(Hit::PomoReset), t0 + secs(6));
        s.apply(Event::Hover(true), t0 + secs(6));
        assert_eq!(s.view(), View::Panel(Page::Calendar));
        s.apply(Event::Hover(false), t0 + secs(6));
        s.apply(Event::Fullscreen(true), t0 + secs(6));
        assert_eq!(s.view(), View::Hidden);
        s.apply(Event::Fullscreen(false), t0 + secs(6));
        assert_eq!(s.view(), View::MediaLyric);
        assert_eq!(s.apply(Event::Click(Hit::Body), t0 + secs(7)), vec![]);
    }

    #[test]
    fn a_new_track_waits_for_its_own_lyrics() {
        let t0 = Instant::now();
        let mut s = st();
        lyric_ready(&mut s, t0);
        let next = MediaInfo {
            title: "Another One".into(),
            ..media(true)
        };
        s.apply(Event::Media(Some(next)), t0);
        assert!(!s.lyrics_ready());
        assert_eq!(s.view(), View::MediaCompact);
    }

    #[test]
    fn lyric_button_flips_the_setting_and_saves() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Media, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::LyricsToggle), t0),
            vec![Effect::SaveLyrics(true)]
        );
        assert!(s.lyrics_on());
        assert_eq!(
            s.apply(Event::Click(Hit::LyricsToggle), t0),
            vec![Effect::SaveLyrics(false)]
        );
        assert!(!s.lyrics_on());
        assert_eq!(s.view(), View::Panel(Page::Media));
    }

    #[test]
    fn the_search_hotkey_opens_it_over_the_page_and_asks_for_the_keyboard() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::SearchClose, t0);
        let effects = s.apply(Event::SearchHotkey, t0);
        assert_eq!(effects, vec![Effect::FocusSearch]);
        assert!(s.search_open());
        assert_eq!(s.view(), View::Panel(Page::Search));
        // Pressed again: it closes, the page it came over is back, and the
        // keyboard goes.
        let effects = s.apply(Event::SearchHotkey, t0);
        assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
        assert!(!s.search_open());
        assert_ne!(s.view(), View::Panel(Page::Search));
        assert_eq!(s.page(), Page::Notes);
    }

    #[test]
    fn the_search_stays_while_open_and_closes_on_close() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::SearchHotkey, t0);
        assert_eq!(s.view(), View::Panel(Page::Search));
        // The pointer coming and going does not close it.
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Hover(false), t0);
        assert_eq!(s.view(), View::Panel(Page::Search));
        // The wheel stays out of it (it is not one of the pages).
        s.apply(Event::Wheel(-120), t0);
        s.apply(Event::Wheel(120), t0);
        assert_eq!(s.view(), View::Panel(Page::Search));
        let effects = s.apply(Event::SearchClose, t0);
        assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
        assert_ne!(s.view(), View::Panel(Page::Search));
        assert_eq!(s.page(), Page::Calendar);
        // Closing what is closed does nothing.
        assert!(s.apply(Event::SearchClose, t0).is_empty());
    }

    #[test]
    fn the_back_button_returns_to_the_page_and_keeps_the_panel_open() {
        let t0 = Instant::now();
        for page in [Page::Notes, Page::Media, Page::Shortcuts] {
            let mut s = st();
            open_on(&mut s, page, t0);
            s.apply(Event::SearchHotkey, t0);
            assert_eq!(s.view(), View::Panel(Page::Search));
            let effects = s.apply(Event::Click(Hit::SearchBack), t0);
            assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
            assert_eq!(s.page(), page);
            assert_eq!(s.view(), View::Panel(page));
            assert!(!s.search_open());
            assert!(s.panel_is_open());
        }
    }

    #[test]
    fn the_back_button_with_the_notch_closed_opens_the_last_page() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        leave(&mut s, t0);
        assert!(!s.panel_is_open());
        s.apply(Event::SearchHotkey, t0);
        assert_eq!(s.view(), View::Panel(Page::Search));
        let effects = s.apply(Event::Click(Hit::SearchBack), t0);
        assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
        assert_eq!(s.view(), View::Panel(Page::Notes));
        assert!(s.panel_is_open());
        // The latch, not the 4 s linger, holds it with the pointer away.
        let later = t0 + ACTIVITY_LINGER + secs(1);
        s.apply(Event::Hover(false), later);
        s.apply(Event::Tick, later);
        assert_eq!(s.view(), View::Panel(Page::Notes));
        assert!(s.panel_is_open());
        // A fresh state: the start page.
        let mut f = st();
        f.apply(Event::SearchHotkey, t0);
        f.apply(Event::Click(Hit::SearchBack), t0);
        assert_eq!(f.view(), View::Panel(Page::Calendar));
        assert!(f.panel_is_open());
        let later = t0 + ACTIVITY_LINGER + secs(1);
        f.apply(Event::Hover(false), later);
        f.apply(Event::Tick, later);
        assert_eq!(f.view(), View::Panel(Page::Calendar));
    }

    #[test]
    fn after_the_back_button_a_click_outside_closes_the_panel() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Media, t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::SearchHotkey, t0);
        s.apply(Event::Click(Hit::SearchBack), t0);
        assert!(s.panel_is_open());
        assert!(s.outside_click_wanted());
        s.apply(Event::Dismiss, t0);
        assert!(!s.panel_is_open());
        assert_ne!(s.view(), View::Panel(Page::Media));
    }

    #[test]
    fn esc_still_collapses_the_panel_unlike_the_back_button() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        s.apply(Event::Hover(false), t0);
        s.apply(Event::SearchHotkey, t0);
        let effects = s.apply(Event::SearchClose, t0);
        assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
        assert!(!s.panel_is_open());
        assert_eq!(s.page(), Page::Notes);
    }

    #[test]
    fn the_back_button_while_the_search_is_closed_does_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        assert!(s.apply(Event::Click(Hit::SearchBack), t0).is_empty());
        assert!(!s.panel_is_open());
        // Open on a page, no search: still nothing.
        open_on(&mut s, Page::Notes, t0);
        assert!(s.apply(Event::Click(Hit::SearchBack), t0).is_empty());
        assert_eq!(s.view(), View::Panel(Page::Notes));
    }

    #[test]
    fn the_back_page_is_never_the_search() {
        assert_eq!(back_page(Page::Search), Page::Shortcuts);
        assert_eq!(back_page(Page::Notes), Page::Notes);
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::SearchHotkey, t0);
        s.search_prev = Page::Search;
        let effects = s.apply(Event::Click(Hit::SearchBack), t0);
        assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
        assert_eq!(s.view(), View::Panel(Page::Shortcuts));
    }

    #[test]
    fn a_result_click_runs_it_and_closes_the_search() {
        let t0 = Instant::now();
        let mut s = st();
        // Closed: a stray click on a row does nothing.
        assert!(s.apply(Event::Click(Hit::SearchResult(0)), t0).is_empty());
        s.apply(Event::SearchHotkey, t0);
        let effects = s.apply(Event::Click(Hit::SearchResult(2)), t0);
        assert_eq!(
            effects,
            vec![Effect::SearchRun(2), Effect::ReleaseSearchFocus]
        );
        assert!(!s.search_open());
        assert_ne!(s.view(), View::Panel(Page::Search));
    }

    #[test]
    fn the_search_hotkey_does_nothing_in_fullscreen_and_never_changes_the_page_order() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        assert!(s.apply(Event::SearchHotkey, t0).is_empty());
        assert!(!s.search_open());
        s.apply(Event::Fullscreen(false), t0);
        // Opened twice over itself, the page to return to is still the real one.
        open_on(&mut s, Page::Media, t0);
        s.apply(Event::SearchHotkey, t0);
        s.apply(Event::SearchClose, t0);
        s.apply(Event::SearchHotkey, t0);
        s.apply(Event::SearchClose, t0);
        assert_eq!(s.page(), Page::Media);
        assert!(!s.pages().visible().contains(&Page::Search));
    }

    #[test]
    fn print_screen_asks_for_the_capture_and_shuts_an_open_panel_or_search_first() {
        let t0 = Instant::now();
        // No panel: the capture is asked for, nothing else changes.
        let mut s = st();
        assert_eq!(s.apply(Event::PrintScreen, t0), vec![Effect::RunCapture]);
        assert!(!matches!(s.view(), View::Panel(_)));
        // An open panel collapses, like the Screenshot tile's click.
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        let effects = s.apply(Event::PrintScreen, t0);
        assert_eq!(effects, vec![Effect::RunCapture]);
        assert!(!matches!(s.view(), View::Panel(_)));
        // The open search closes (and lets go of the keyboard) too.
        let mut s = st();
        s.apply(Event::SearchHotkey, t0);
        assert!(s.search_open());
        let effects = s.apply(Event::PrintScreen, t0);
        assert!(effects.contains(&Effect::RunCapture));
        assert!(!s.search_open());
        assert!(!matches!(s.view(), View::Panel(_)));
    }

    #[test]
    fn print_screen_during_a_notice_still_shuts_the_panel_held_under_it() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        // The hovered panel stays held while a notice shows over it.
        s.apply(Event::Notice(app_notice("k1")), t0);
        assert_eq!(s.view(), View::Notice(app_notice("k1")));
        assert!(s.panel_open(), "the panel is held under the notice");
        assert_eq!(s.apply(Event::PrintScreen, t0), vec![Effect::RunCapture]);
        // Shut, so it cannot come back once the notice ends and be in the
        // picture.
        assert!(!s.panel_open());
        s.apply(Event::Tick, t0 + Duration::from_secs(60));
        assert!(!matches!(s.view(), View::Panel(_)));
    }

    #[test]
    fn the_clipboard_hotkey_over_the_search_closes_it() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::SearchHotkey, t0);
        let effects = s.apply(Event::ClipHotkey, t0);
        assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
        assert!(!s.search_open());
        assert_eq!(s.view(), View::Panel(Page::Clipboard), "the new page stays");
        // Opened again, it comes back to the Clipboard page.
        s.apply(Event::SearchHotkey, t0);
        assert_eq!(
            s.apply(Event::SearchClose, t0),
            vec![Effect::ReleaseSearchFocus]
        );
        assert_eq!(s.page(), Page::Clipboard);
    }

    #[test]
    fn files_dragged_over_the_search_close_it() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::SearchHotkey, t0);
        let effects = s.apply(Event::ShelfDrag(true), t0);
        assert_eq!(effects, vec![Effect::ReleaseSearchFocus]);
        assert!(!s.search_open());
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        // The drag ends away from the island: the panel shuts, no search.
        assert!(s.apply(Event::ShelfDrag(false), t0).is_empty());
        assert!(!matches!(s.view(), View::Panel(_)));
    }

    #[test]
    fn a_mole_strip_click_over_the_search_closes_it_on_the_log() {
        let t0 = Instant::now();
        let mut s = st();
        let failed = mole_notice(NoticeKind::MoleFailed);
        s.apply(Event::Notice(failed.clone()), t0);
        s.apply(Event::Notice(failed.clone()), t0);
        s.apply(Event::Hover(true), t0);
        assert_eq!(s.waiting_strip(), Some(&failed));
        s.apply(Event::SearchHotkey, t0);
        assert_eq!(s.view(), View::Panel(Page::Search));
        let effects = s.apply(Event::Click(Hit::Strip), t0);
        assert!(effects.contains(&Effect::ReleaseSearchFocus), "{effects:?}");
        assert!(!s.search_open());
        assert_eq!(s.view(), View::Panel(Page::System));
        assert_eq!(s.cleaner().unwrap().mode, CleanMode::Mole);
    }

    #[test]
    fn the_search_takes_the_keyboard_from_the_cli_and_clipboard_fields_without_handing_it_back() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Cli, t0);
        s.apply(Event::CliFocus(true), t0);
        // No ReleaseCliFocus after FocusSearch: that would hand the keyboard
        // to the window before and close the search at once.
        let effects = s.apply(Event::SearchHotkey, t0);
        assert_eq!(effects, vec![Effect::FocusSearch]);
        assert!(s.search_open() && !s.cli_focused());
        assert_eq!(
            s.apply(Event::SearchClose, t0),
            vec![Effect::ReleaseSearchFocus]
        );
        assert_eq!(s.page(), Page::Cli);

        let mut s = st();
        open_on(&mut s, Page::Clipboard, t0);
        s.apply(Event::ClipFocus(true), t0);
        let effects = s.apply(Event::SearchHotkey, t0);
        assert_eq!(effects, vec![Effect::FocusSearch]);
        assert!(s.search_open() && !s.clip_focused());
        assert_eq!(s.view(), View::Panel(Page::Search));
    }

    #[test]
    fn the_tray_opens_the_search_or_focuses_it_but_never_closes_it() {
        let t0 = Instant::now();
        let mut s = st();
        assert_eq!(s.apply(Event::SearchOpen, t0), vec![Effect::FocusSearch]);
        assert!(s.search_open());
        // Open already: the field takes the keyboard again, it stays open.
        assert_eq!(s.apply(Event::SearchOpen, t0), vec![Effect::FocusSearch]);
        assert!(s.search_open());
        assert_eq!(s.view(), View::Panel(Page::Search));
        // Hidden under a fullscreen app: nothing.
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        assert!(s.apply(Event::SearchOpen, t0).is_empty());
        assert!(!s.search_open());
    }

    #[test]
    fn a_custom_tile_collapses_the_panel_and_runs_that_tile() {
        use crate::tiles::{CustomKind, CustomTile};
        let t0 = Instant::now();
        let mut s = st();
        let own = CustomTile::new("Gmail", CustomKind::Url, "https://mail.google.com").unwrap();
        let mut tiles = ShortcutTiles::default();
        tiles.toggle(0);
        tiles.add(own.clone());
        s.apply(Event::ShortcutTiles(tiles), t0);
        // Shown: the default tiles but one, then Gmail right after them.
        let own_at = ShortcutTiles::default().shown() - 1;
        assert_eq!(s.tiles().len(), own_at + 1);
        open_on(&mut s, Page::Shortcuts, t0);
        let effects = s.apply(Event::Click(Hit::CustomTile(own_at)), t0);
        assert_eq!(effects, vec![Effect::RunCustom(own)]);
        // A place that is not a custom tile, or past the end: nothing.
        let mut s2 = st();
        s2.apply(Event::ShortcutTiles(ShortcutTiles::default()), t0);
        open_on(&mut s2, Page::Shortcuts, t0);
        assert!(s2.apply(Event::Click(Hit::CustomTile(0)), t0).is_empty());
        assert!(s2.apply(Event::Click(Hit::CustomTile(40)), t0).is_empty());
    }

    #[test]
    fn files_dragged_over_the_island_open_the_shelf_and_hold_it_until_they_leave() {
        let t0 = Instant::now();
        let mut s = st();
        assert!(!matches!(s.view(), View::Panel(_)));
        s.apply(Event::ShelfDrag(true), t0);
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        // They leave without a drop: the panel goes with them.
        s.apply(Event::ShelfDrag(false), t0);
        assert!(!matches!(s.view(), View::Panel(_)));
        // Dropped and the pointer arrives: hover holds the shelf, and the end
        // of the drop's hold does not close it.
        s.apply(Event::ShelfDrag(true), t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::ShelfDrag(false), t0);
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        // The reducer ignores the paths themselves: `App` keeps them.
        let effects = s.apply(Event::ShelfDrop(vec!["a.txt".into()]), t0);
        assert!(effects.is_empty());
        // A game on top: nothing opens.
        let mut g = st();
        g.apply(Event::Fullscreen(true), t0);
        g.apply(Event::ShelfDrag(true), t0);
        assert!(!matches!(g.view(), View::Panel(_)));
    }

    // ---- v3.3: a drop opens the shelf and keeps it ----

    #[test]
    fn a_drop_keeps_the_shelf_open_after_the_drag_ends() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::ShelfDrag(true), t0);
        s.apply(Event::ShelfDropped, t0);
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        // The drag's own end (an empty drop, the 600 ms leave) and a long
        // wait change nothing: the drop latched the panel.
        s.apply(Event::ShelfDrag(false), t0);
        s.apply(Event::Tick, t0 + secs(60));
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        assert!(s.outside_click_wanted());
    }

    #[test]
    fn the_drop_visit_leaving_does_not_close_it() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Hover, t0);
        s.apply(Event::ShelfDropped, t0);
        s.apply(Event::Hover(true), t0 + secs(1));
        s.apply(Event::Hover(false), t0 + secs(2));
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        assert!(s.outside_click_wanted());
    }

    #[test]
    fn a_leave_before_the_dwell_also_ends_the_drop_visit() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Hover, t0);
        assert!(!s.shelf_drop_here());
        s.apply(Event::ShelfDropped, t0);
        assert!(s.shelf_drop_here());
        // The pointer left before the dwell: no Hover(false) will come.
        s.apply(Event::ShelfDropLeft, t0 + secs(1));
        assert!(!s.shelf_drop_here());
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        // The next visit is an ordinary one: hover holds it, leaving closes it.
        s.apply(Event::Hover(true), t0 + secs(2));
        s.apply(Event::Hover(false), t0 + secs(3));
        assert!(!matches!(s.view(), View::Panel(_)));
    }

    #[test]
    fn after_a_drop_a_new_visit_hands_the_panel_to_hover() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Hover, t0);
        s.apply(Event::ShelfDropped, t0);
        s.apply(Event::Hover(true), t0 + secs(1));
        s.apply(Event::Hover(false), t0 + secs(2));
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        // Back over the panel: hover holds it now, no click outside needed.
        s.apply(Event::Hover(true), t0 + secs(3));
        assert_eq!(s.view(), View::Panel(Page::Shelf));
        assert!(!s.outside_click_wanted());
        s.apply(Event::Hover(false), t0 + secs(4));
        assert!(!matches!(s.view(), View::Panel(_)));
    }

    #[test]
    fn a_click_outside_closes_the_dropped_shelf() {
        let t0 = Instant::now();
        let mut s = mode(OpenMode::Hover, t0);
        s.apply(Event::ShelfDropped, t0);
        s.apply(Event::Dismiss, t0 + secs(1));
        assert!(!matches!(s.view(), View::Panel(_)));
        assert!(!s.outside_click_wanted());
        // Under the pointer it stays shut until the pointer comes back.
        let mut s = mode(OpenMode::Hover, t0);
        s.apply(Event::ShelfDropped, t0);
        s.apply(Event::Hover(true), t0);
        s.apply(Event::Dismiss, t0 + secs(1));
        assert!(!matches!(s.view(), View::Panel(_)));
        s.apply(Event::Hover(false), t0 + secs(2));
        assert!(!matches!(s.view(), View::Panel(_)));
        s.apply(Event::Hover(true), t0 + secs(3));
        assert_eq!(s.view(), View::Panel(Page::Shelf));
    }

    #[test]
    fn a_drop_in_click_or_hover_stay_mode_stays_until_a_click_outside() {
        let t0 = Instant::now();
        for m in [OpenMode::HoverStay, OpenMode::Click] {
            let mut s = mode(m, t0);
            s.apply(Event::ShelfDropped, t0);
            assert_eq!(s.view(), View::Panel(Page::Shelf), "{m:?}");
            for i in 1..=3u64 {
                s.apply(Event::Hover(true), t0 + secs(2 * i));
                s.apply(Event::Hover(false), t0 + secs(2 * i + 1));
                assert_eq!(s.view(), View::Panel(Page::Shelf), "{m:?}");
                assert!(s.outside_click_wanted(), "{m:?}");
            }
            s.apply(Event::Dismiss, t0 + secs(20));
            assert!(!matches!(s.view(), View::Panel(_)), "{m:?}");
        }
    }

    #[test]
    fn a_card_click_or_page_change_ends_the_drop_hold() {
        let t0 = Instant::now();
        // A card opens its file and collapses the panel, latch and all.
        let mut s = mode(OpenMode::Hover, t0);
        s.apply(Event::ShelfDropped, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::ShelfCard(0)), t0 + secs(1)),
            vec![Effect::ShelfOpen(0)]
        );
        assert!(!matches!(s.view(), View::Panel(_)));
        assert!(!s.latched);
        // A page turned with the pointer on it hands the panel to hover.
        let mut s = mode(OpenMode::Hover, t0);
        s.apply(Event::ShelfDropped, t0);
        s.apply(Event::Hover(true), t0 + secs(1));
        s.apply(Event::Wheel(-120), t0 + secs(2));
        assert_ne!(s.page(), Page::Shelf);
        assert!(!s.latched);
        assert!(!s.shelf_drop_here());
        leave(&mut s, t0 + secs(3));
        assert!(!matches!(s.view(), View::Panel(_)));
    }

    #[test]
    fn a_drop_during_fullscreen_opens_nothing() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::ShelfDropped, t0);
        assert!(!matches!(s.view(), View::Panel(_)));
        // Nothing was latched behind the game.
        s.apply(Event::Fullscreen(false), t0 + secs(1));
        assert!(!matches!(s.view(), View::Panel(_)));
        assert!(!s.outside_click_wanted());
    }

    #[test]
    fn the_drop_targets_own_report_is_for_app_and_the_reducer_ignores_it() {
        let t0 = Instant::now();
        let mut s = st();
        assert!(s.apply(Event::ShelfDragOver(true), t0).is_empty());
        assert!(
            !matches!(s.view(), View::Panel(_)),
            "App turns it into ShelfDrag"
        );
    }

    #[test]
    fn the_shelf_cards_work_only_while_its_page_is_open() {
        let t0 = Instant::now();
        let mut s = st();
        open_on(&mut s, Page::Notes, t0);
        for hit in [Hit::ShelfCard(0), Hit::ShelfRemove(0), Hit::ShelfClear] {
            assert!(s.apply(Event::Click(hit), t0).is_empty());
        }
        open_on(&mut s, Page::Shelf, t0);
        assert_eq!(
            s.apply(Event::Click(Hit::ShelfRemove(2)), t0),
            vec![Effect::ShelfRemove(2)]
        );
        assert_eq!(
            s.apply(Event::Click(Hit::ShelfClear), t0),
            vec![Effect::ShelfClear]
        );
        // A card opens its file once the panel is out of the way.
        assert_eq!(
            s.apply(Event::Click(Hit::ShelfCard(1)), t0),
            vec![Effect::ShelfOpen(1)]
        );
        assert!(!s.panel_is_open());
    }

    #[test]
    fn timeline_is_kept_and_rebased_on_play_pause() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Media(Some(media(true))), t0);
        let t = Timeline {
            position_ms: 10_000,
            end_ms: 200_000,
            updated: t0,
            rate: 1.0,
        };
        s.apply(Event::MediaTimeline(Some(t)), t0);
        assert_eq!(s.timeline(), Some(&t));
        // Paused 3 s later: the position freezes there.
        let paused = t0 + secs(3);
        s.apply(Event::Media(Some(media(false))), paused);
        let frozen = Timeline {
            position_ms: 13_000,
            end_ms: 200_000,
            updated: paused,
            rate: 1.0,
        };
        assert_eq!(s.timeline(), Some(&frozen));
        // A minute later it resumes from there.
        let resumed = paused + secs(60);
        s.apply(Event::Media(Some(media(true))), resumed);
        assert_eq!(
            s.timeline(),
            Some(&Timeline {
                updated: resumed,
                rate: 1.0,
                ..frozen
            })
        );
        // A fresh timeline from the watcher wins.
        s.apply(Event::MediaTimeline(Some(t)), resumed);
        assert_eq!(s.timeline(), Some(&t));
        s.apply(Event::MediaTimeline(None), resumed);
        assert_eq!(s.timeline(), None);
    }

    // ---- The screenshot preview (Run 8) ----

    /// Screenshot `id`'s preview: 8 s, the file kept when ignored (the
    /// default) or deleted (`delete`, the opt-in).
    fn shot(id: u64, delete: bool) -> Notice {
        Notice {
            key: format!("shot-{id}"),
            source: "Screenshot".into(),
            title: "Screenshot".into(),
            body: String::new(),
            kind: NoticeKind::Screenshot {
                shot: id,
                secs: 8,
                delete_if_ignored: delete,
            },
        }
    }

    fn deletes(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Effect::Shot(ShotAction::Delete(_) | ShotAction::DeleteIgnored(_))
                )
            })
            .count()
    }

    /// Every effect of `events` applied in order at their times.
    fn run(s: &mut IslandState, events: Vec<(Event, Instant)>) -> Vec<Effect> {
        events
            .into_iter()
            .flat_map(|(e, at)| s.apply(e, at))
            .collect()
    }

    #[test]
    fn an_ignored_preview_goes_after_eight_seconds_and_keeps_the_file() {
        let t0 = Instant::now();
        let mut s = st();
        assert!(s.apply(Event::Notice(shot(1, false)), t0).is_empty());
        assert_eq!(s.view(), View::Notice(shot(1, false)));
        assert!(s.has_shot(1));
        assert_eq!(s.next_deadline(), Some(t0 + secs(8)));
        assert!(s.apply(Event::Tick, t0 + secs(7)).is_empty());
        assert_eq!(s.view(), View::Notice(shot(1, false)));
        let fx = s.apply(Event::Tick, t0 + secs(8));
        assert_eq!(deletes(&fx), 0, "Simpan is the default: kept");
        assert_eq!(s.view(), View::Idle);
        assert!(!s.has_shot(1));
    }

    #[test]
    fn the_default_never_deletes_without_the_hapus_click() {
        let t0 = Instant::now();
        // Timeout, Body click, Salin lagi, Simpan, a newer shot, fullscreen,
        // the pointer holding it: none of them asks for a delete.
        let cases: Vec<Vec<(Event, Duration)>> = vec![
            vec![(Event::Tick, secs(30))],
            vec![(Event::Click(Hit::Body), secs(1))],
            vec![(Event::Click(Hit::ShotCopy(1)), secs(1))],
            vec![(Event::Click(Hit::ShotKeep(1)), secs(1))],
            vec![(Event::Click(Hit::ShotOpen(1)), secs(1))],
            vec![
                (Event::Notice(shot(2, false)), secs(2)),
                (Event::Tick, secs(60)),
            ],
            vec![(Event::Fullscreen(true), secs(1)), (Event::Tick, secs(60))],
            vec![
                (Event::Hover(true), secs(1)),
                (Event::Tick, secs(30)),
                (Event::Hover(false), secs(31)),
                (Event::Tick, secs(40)),
            ],
        ];
        for case in cases {
            let mut s = st();
            let mut fx = s.apply(Event::Notice(shot(1, false)), t0);
            fx.extend(run(
                &mut s,
                case.iter().map(|(e, d)| (e.clone(), t0 + *d)).collect(),
            ));
            assert_eq!(deletes(&fx), 0, "{case:?}");
        }
        // Only the Hapus click.
        let mut s = st();
        s.apply(Event::Notice(shot(1, false)), t0);
        let fx = s.apply(Event::Click(Hit::ShotDelete(1)), t0 + secs(1));
        assert_eq!(fx, [Effect::Shot(ShotAction::Delete(1))]);
        assert_eq!(s.view(), View::Idle);
    }

    #[test]
    fn buttons_close_the_preview_and_name_their_action() {
        let t0 = Instant::now();
        for (hit, action) in [
            (Hit::ShotCopy(1), ShotAction::Copy(1)),
            (Hit::ShotKeep(1), ShotAction::Keep(1)),
        ] {
            let mut s = st();
            s.apply(Event::Notice(shot(1, true)), t0);
            assert_eq!(
                s.apply(Event::Click(hit), t0 + secs(1)),
                [Effect::Shot(action)]
            );
            assert!(!s.has_shot(1));
            // Long after: nothing more, the opt-in delete included.
            assert!(s.apply(Event::Tick, t0 + secs(60)).is_empty());
        }
        // A button of another (older) shot does nothing.
        let mut s = st();
        s.apply(Event::Notice(shot(2, false)), t0);
        assert!(s.apply(Event::Click(Hit::ShotDelete(1)), t0).is_empty());
        assert!(s.has_shot(2));
        // The thumbnail opens the file and the preview stays.
        assert_eq!(
            s.apply(Event::Click(Hit::ShotOpen(2)), t0),
            [Effect::Shot(ShotAction::Open(2))]
        );
        assert!(s.has_shot(2));
    }

    #[test]
    fn hapus_on_ignore_deletes_only_after_the_full_time() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(shot(1, true)), t0);
        assert_eq!(deletes(&s.apply(Event::Tick, t0 + secs(7))), 0);
        let fx = s.apply(Event::Tick, t0 + secs(8));
        assert_eq!(fx, [Effect::Shot(ShotAction::DeleteIgnored(1))]);
        assert_eq!(s.view(), View::Idle);
        assert!(s.apply(Event::Tick, t0 + secs(20)).is_empty(), "once");
        // Never when it did not run its time untouched.
        let cases: Vec<Vec<(Event, Duration)>> = vec![
            vec![(Event::Click(Hit::Body), secs(3))],
            vec![(Event::Notice(shot(2, false)), secs(3))],
            vec![(Event::Fullscreen(true), secs(3))],
            vec![
                (Event::Hover(true), secs(3)),
                (Event::Hover(false), secs(4)),
            ],
            vec![(Event::Click(Hit::ShotOpen(1)), secs(3))],
        ];
        for case in cases {
            let mut s = st();
            let mut fx = s.apply(Event::Notice(shot(1, true)), t0);
            fx.extend(run(
                &mut s,
                case.iter().map(|(e, d)| (e.clone(), t0 + *d)).collect(),
            ));
            fx.extend(s.apply(Event::Tick, t0 + secs(60)));
            assert_eq!(deletes(&fx), 0, "{case:?}");
        }
    }

    #[test]
    fn the_preview_waits_for_its_turn_and_counts_from_then() {
        let t0 = Instant::now();
        let mut s = st();
        let other = Notice {
            kind: NoticeKind::CleanDone,
            ..copied("x")
        };
        s.apply(Event::Notice(other.clone()), t0);
        s.apply(Event::Notice(shot(1, true)), t0 + secs(1));
        // It never removes another kind of notice.
        assert_eq!(s.view(), View::Notice(other));
        assert!(s.has_shot(1));
        // Current at 5 s, so it ends at 13 s.
        s.apply(Event::Tick, t0 + secs(5));
        assert_eq!(s.view(), View::Notice(shot(1, true)));
        assert_eq!(deletes(&s.apply(Event::Tick, t0 + secs(12))), 0);
        assert_eq!(deletes(&s.apply(Event::Tick, t0 + secs(13))), 1);
    }

    #[test]
    fn the_pointer_holds_the_preview_then_two_seconds_more() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(shot(1, false)), t0);
        s.apply(Event::Hover(true), t0 + secs(6));
        assert_eq!(s.next_deadline(), None, "no expiry under the pointer");
        s.apply(Event::Tick, t0 + secs(20));
        assert_eq!(s.view(), View::Notice(shot(1, false)));
        // A click right before leaving still lands.
        s.apply(Event::Hover(false), t0 + secs(21));
        assert_eq!(s.next_deadline(), Some(t0 + secs(23)));
        s.apply(Event::Tick, t0 + secs(22));
        assert_eq!(s.view(), View::Notice(shot(1, false)));
        s.apply(Event::Tick, t0 + secs(23));
        assert_eq!(s.view(), View::Idle);
        // Leaving early keeps the full time.
        let mut s = st();
        s.apply(Event::Notice(shot(1, false)), t0);
        s.apply(Event::Hover(true), t0 + secs(1));
        s.apply(Event::Hover(false), t0 + secs(2));
        assert_eq!(s.next_deadline(), Some(t0 + secs(8)));
    }

    #[test]
    fn a_newer_screenshot_replaces_previews_and_copied_only() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Notice(copied("x")), t0);
        s.apply(Event::Notice(shot(1, false)), t0);
        assert_eq!(s.view(), View::Notice(shot(1, false)), "over Disalin");
        s.apply(Event::Notice(shot(2, false)), t0 + secs(1));
        assert_eq!(s.view(), View::Notice(shot(2, false)));
        assert!(!s.has_shot(1));
        // Counted from when it became current.
        assert_eq!(s.next_deadline(), Some(t0 + secs(9)));
        // A reminder (sticky) stays underneath.
        s.apply(Event::Notice(reminder("r1")), t0 + secs(2));
        s.apply(Event::Tick, t0 + secs(9));
        assert_eq!(s.view(), View::Notice(reminder("r1")));
    }

    #[test]
    fn never_over_a_fullscreen_app() {
        let t0 = Instant::now();
        let mut s = st();
        s.apply(Event::Fullscreen(true), t0);
        s.apply(Event::Notice(shot(1, true)), t0);
        assert_eq!(s.view(), View::Hidden);
        assert!(!s.has_shot(1));
        // Hidden by a game: it goes, kept.
        let mut s = st();
        s.apply(Event::Notice(shot(1, true)), t0);
        let fx = s.apply(Event::Fullscreen(true), t0 + secs(2));
        assert_eq!(deletes(&fx), 0);
        assert_eq!(s.view(), View::Hidden);
        assert!(!s.has_shot(1));
    }

    #[test]
    fn saved_and_deleted_notices_are_short() {
        let t0 = Instant::now();
        let mut s = st();
        let saved = Notice {
            kind: NoticeKind::ShotSaved,
            ..copied("x")
        };
        s.apply(Event::Notice(saved.clone()), t0);
        assert_eq!(s.view(), View::Notice(saved));
        assert_eq!(s.next_deadline(), Some(t0 + SHOT_DONE_DURATION));
    }
}
