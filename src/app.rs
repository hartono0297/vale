//! The running island. One UI thread owns the window, renderer and state;
//! background sources queue `Event`s on a channel and post `WM_APP_EVENT` to
//! wake it. Nothing redraws unless an event arrives, an animation runs, or a
//! visible live element (equalizer, pomodoro countdown) needs its next frame.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Power::{
    HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    UnregisterPowerSettingNotification,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
    WTS_SESSIONSTATE_UNLOCK, WTSFreeMemory, WTSINFOEXW, WTSQuerySessionInformationW,
    WTSRegisterSessionNotification, WTSSessionInfoEx, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, TerminateProcess};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO, ReleaseCapture, SetCapture, SetFocus,
    TME_HOVER, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VIRTUAL_KEY, VK_LBUTTON, VK_MBUTTON,
    VK_RBUTTON,
};
use windows::Win32::UI::Controls::{WM_MOUSEHOVER, WM_MOUSELEAVE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    DBT_DEVNODES_CHANGED, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow, DispatchMessageW, EN_CHANGE,
    GA_ROOT, GetAncestor, GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics,
    IsWindow, IsWindowVisible, KillTimer, MA_NOACTIVATE, MSG, PBT_POWERSETTINGCHANGE, PM_REMOVE,
    PeekMessageW, PostMessageW, PostQuitMessage, SM_SWAPBUTTON, SPI_GETCLIENTAREAANIMATION,
    SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCoalescableTimer, SetForegroundWindow,
    SetTimer, SystemParametersInfoW, TranslateMessage, WA_INACTIVE, WM_ACTIVATE, WM_APP,
    WM_CAPTURECHANGED, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_DEVICECHANGE,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WM_WTSSESSION_CHANGE,
    WM_XBUTTONDOWN, WindowFromPoint,
};
use windows::core::{PCWSTR, PWSTR, w};

use crate::arrange_overlay;
use crate::arrange_win;
use crate::audio;
use crate::behind;
use crate::calendar::sync::{SyncHandle, SyncRequest, post_status, save_failed_notice, spawn_sync};
use crate::calendar::{CalEvent, CalStatus, CalendarSnapshot, draft_from, new_draft};
use crate::claude::pipe_server::{PipeServer, spawn_pipe_server};
use crate::clean::{self, CleanCat, CleanEvent, Cleaner, ScanResult};
use crate::cli::{self, Builtin, CliEvent, History, HistoryCursor, RunHandle, Session};
use crate::cli_folders::{self, FolderList, ListKind};
use crate::cli_panel::{self, Echoes, MAX_SUGGESTIONS, RunClock};
use crate::config::{self, Config, NotchMonitor, OnDisk, Repair, Style};
use crate::data_dir;
use crate::eq_audio::EqCapture;
use crate::gestures;
use crate::google::client::{self, Client};
use crate::greeting::{DayPeriod, GreetCounts, GreetMark};
use crate::halo;
use crate::i18n::{self, Key, tr};
use crate::identify_overlay;
use crate::lyrics::{self, DiskCache, LyricsState, LyricsWorker, Query};
use crate::media_log;
use crate::model::{Effect, Event, Hit, Notice, NoticeKind, Page, RecKind, RecSources, View};
use crate::mole::{self, Confirmed, MoleEvent, MoleOpts, MoleWorker};
use crate::notch_monitor::{self, Display};
use crate::notes::NotesStore;
use crate::pages::PanelPages;
use crate::pointer_watch::{
    self, EyesLook, EyesMotion, GlanceRest, LookEase, Peek, PeekInputs, PeekState, UnderCache,
};
use crate::pomodoro::PomoStatus;
use crate::reminders::{self, Reminder, RemindersStore, due_now, next_due};
use crate::shelf_ole;
use crate::shortcuts;
use crate::sources::battery::{
    BatteryMonitor, battery_low_now, battery_notice, energy_saver_on, read_battery,
};
use crate::sources::fullscreen::is_fullscreen;
use crate::sources::{explorer, media, notif_open, notifications};
use crate::state::{IslandState, ShownPanel, ToastForm, is_media_hit, is_real_toast};
use crate::system::{
    CpuHistory, SampleKind, Sampler, SysEase, SystemSample, WarnMonitor, alert_notice,
};
use crate::theme::{self, Mascot, MascotPlay, Theme};
use crate::tiles::ShortcutTiles;
use crate::ui::app_icon;
use crate::ui::cat_style::CatLook;
use crate::ui::dirty::{self, EqKey};
use crate::ui::cli_input::{self, CliInput, Key as InputKey, WM_APP_CLI_REDRAW};
use crate::ui::editor::{self, EditorKind, EditorResult, WM_APP_EDITOR_RESULT};
use crate::ui::frame_clock::{FrameClock, WM_APP_FRAME};
use crate::ui::geometry::{
    AA_MARGIN, ANIM_MS, LYRIC_HOLD_MS, LYRIC_MAX_W, LYRIC_MIN_W, NOTCH_H_DEFAULT, NOTCH_W_DEFAULT,
    NotchSize, ShapeKind, ShapeRect, TOAST_SLOT_EXTRA, Under, WINDOW_W, idle_shape_typing,
    island_contains, lerp_shape, lyric_box_w, lyric_cap, lyric_max_w, lyric_scroll_end_ms,
    lyric_scroll_offset, lyric_shape_sized, lyric_slot_width, lyric_target_w, lyric_width_min,
    panel_kind, shape_for, shape_sized, shape_with_robot_sized, toast_peek_reaches, typing_lead,
    with_rec_caption, with_toast_slot,
};
use crate::ui::motion::{
    CloseFade, FadeStart, MorphCurve, PAGE_FADE_MS, Reveal, SlideStrip, Snap, StripDir,
    StripMotion, StripSnap, SubView, Transit, Transition, close_fade_ms, content_alpha_from,
    ease_in_out_cubic, fade_floor, mole_code, morph_timing, page_fade_frame, panel_fade, strip_dir,
    transition,
};
use crate::ui::notch_fill;
use crate::ui::panel_layout::{CliHead, STRIP_H, cli_layout};
use crate::ui::panel_tips::{self, TipCtx};
use crate::ui::tooltip;
use crate::ui::render::{
    CleanerView, CliView, DevicesView, FolderMenuView, Frame, MoleView, NoticeCloseView,
    PlayFrame, Progress, RecFrame, RecordView, Renderer, RobotFrame, SearchView, SystemView,
};
use crate::ui::play::{click_through, swallows};
use crate::ui::play::gate::{Env, instant_stop, may_start, must_stop};
use crate::ui::play::state::Play;
use crate::ui::play::stories;
use crate::ui::play::trigger::{Moment, PRESENCE_MIN, Story, cooldown, has_ready, pick, wait_for};
use crate::ui::settings_layout::{CardId, MonitorRow};
use crate::ui::robot::{
    self, Brain, Expr, EyesBlink, EyesPose, GreetSettings, Motion, Rng, Spot, TypingPaws,
    choose_expr, plan_timers, should_greet,
};
use crate::ui::settings::{self, Action as SettingsAction, WM_APP_SETTINGS_ACTION};
use crate::ui::tray::{self, MenuChoice, MenuCmd, WM_APP_TRAY};
use crate::ui::window::IslandWindow;
use crate::widgets::host::WM_APP_WIDGET;

#[path = "app_clip.rs"]
mod clip;
use clip::{ClipSetting, ClipState, SEARCH_LIMIT};

#[path = "app_devices.rs"]
mod devices_page;
use devices_page::{DevicesState, TIMER_DEVICES};

#[path = "app_arrange.rs"]
mod arrange_page;
use arrange_page::ArrangeState;

#[path = "app_dock.rs"]
mod dock_page;
use dock_page::DockState;

#[path = "app_notif.rs"]
mod notif_page;
use notif_page::{NotifSetting, NotifState, TIMER_NOTIF_SAVE};

#[path = "app_transfer.rs"]
mod transfer_page;
use transfer_page::Transfer;

#[path = "app_update.rs"]
mod update_page;
use update_page::{TIMER_UPDATE, UpdateState};

#[path = "app_record.rs"]
pub(crate) mod record;
#[path = "app_screenrec.rs"]
pub(crate) mod screen_page;
use record::RecState;
use screen_page::{ScreenState, TIMER_SCREEN_COUNTDOWN, WM_APP_SCREEN_WATCH};

#[path = "app_transcribe.rs"]
mod transcribe_page;
use transcribe_page::{TIMER_TRANSCRIBE, TranscribeState};

#[path = "app_transcript_win.rs"]
mod transcript_page;

#[path = "app_lyrics_card.rs"]
mod card;
use card::LyricsCardState;

#[path = "app_captions.rs"]
mod captions_page;
use captions_page::CaptionState;

#[path = "app_prtsc.rs"]
mod prtsc;
use prtsc::WM_APP_PRTSC;

#[path = "app_hotkeys.rs"]
mod hotkeys;
use hotkeys::{HotkeyState, is_countdown_esc, row_of_wm_id};

#[path = "app_search.rs"]
mod quick;
use quick::{QUERY_LIMIT, SearchState};

#[path = "app_shelf.rs"]
mod shelf_page;
use shelf_page::{ShelfState, TIMER_SHELF_HOLD};

#[path = "app_phone.rs"]
mod phone_page;
use phone_page::{PhoneState, TIMER_PHONE};

#[path = "app_widgets.rs"]
mod widgets_page;
use widgets_page::{TIMER_STICKY_REMIND, TIMER_WIDGET_SAVE, WidgetsState};

#[path = "app_note_list.rs"]
mod note_list;

#[path = "app_shot.rs"]
pub(crate) mod shot;
use shot::{ShotSetting, Shots};

/// Posted by background sources after queueing an event.
const WM_APP_EVENT: u32 = WM_APP + 1;
/// Posted once the user has answered the notification-access prompt.
const WM_APP_ACCESS: u32 = WM_APP + 3;
/// Opens a source's device menu from the main loop, outside the `APP` borrow
/// (`wparam`: 0 mic, 1 PC audio).
const WM_APP_REC_DEVICE: u32 = WM_APP + 80;
/// Opens the Rekam page's video quality menu from the main loop, outside the
/// `APP` borrow.
const WM_APP_REC_QUALITY: u32 = WM_APP + 95;
/// The system clock or time zone changed.
const WM_TIMECHANGE: u32 = 0x001E;
const WM_POWERBROADCAST: u32 = 0x0218;
/// `WM_POWERBROADCAST` codes for waking from sleep (with or without a user).
const PBT_APMRESUMESUSPEND: usize = 0x0007;
/// The PC is about to sleep: the phone link ends.
const PBT_APMSUSPEND: usize = 0x0004;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
/// The power source or battery state changed (Energy Saver too).
const PBT_APMPOWERSTATUSCHANGE: usize = 0x000A;
/// `WM_WTSSESSION_CHANGE`: the session was locked, or the user unlocked it.
const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;
/// How long the pointer must rest on the island before it expands. Long
/// enough that a pointer crossing the notch on its way elsewhere doesn't
/// open it, short enough that a deliberate hover feels immediate.
const HOVER_DWELL_MS: u32 = 120;

const TIMER_SYSTEM: usize = 1;
const TIMER_BATTERY: usize = 2;
const TIMER_DEADLINE: usize = 4;
/// Equalizer frames, only while the compact media view shows a playing track.
const TIMER_EQ: usize = 5;
/// Pomodoro countdown redraws, only while it runs and is on screen.
const TIMER_POMO: usize = 6;
/// Date rollover check.
const TIMER_DAY: usize = 7;
/// One-shot to the next whole second of the REC clock, re-armed after each
/// redraw, only while the REC view (or the Rekam page) shows a recording that
/// is not paused (`rec_clock_timer_wanted`).
const TIMER_REC: usize = 60;
/// One-shot, `CAPTION_LINE_MS` after the newest caption text reached the
/// notch, re-armed by each new text: the line goes away after that much
/// silence (`Event::CaptionLine(None)`). Killed with captions or the
/// notch line off. Never repeats.
const TIMER_CAPTION_LINE: usize = 66;
const CAPTION_LINE_MS: u32 = 8_000;
/// One-shot, `REC_LEVEL_MS` apart (coalescable, re-armed after each redraw):
/// the REC view's level meters redraw, only while that view is on screen at a
/// screen somebody can see, the recording runs and Motion is not Off
/// (`rec_level_timer_wanted`).
const TIMER_REC_LEVEL: usize = 61;
const REC_LEVEL_MS: u32 = 120;
/// Reminders: one-shot at the earliest due time (at most `REMINDER_MAX_MS` away).
const TIMER_REMINDER: usize = 9;
/// Idle clock redraws at each minute boundary, only while it shows.
const TIMER_CLOCK: usize = 8;
/// One-shot to the next lyric line, only while the lyric view shows.
const TIMER_LYRIC: usize = 10;
/// One-shot: a clicked shortcut runs once the panel is out of the way.
const TIMER_SHORTCUT: usize = 11;
/// One-shot to the track position's next whole second, only while the Music
/// page shows a playing track of known length.
const TIMER_PROGRESS: usize = 12;
/// The CLI chip's running time, each second, only while the CLI page shows
/// a running command.
const TIMER_CLI: usize = 13;
/// Bata's blink: one-shot at a random 4–7 s, then 120 ms with the eyes shut.
/// Only while a robot shows and motion is Full; never while animating.
const TIMER_BLINK: usize = 14;
/// One-shot to the next moment Bata's face changes by itself (falling
/// asleep, a Claude session going quiet, a CLI face ending).
const TIMER_ROBOT: usize = 15;
/// One-shot, a random `wait_for(mascot_play)` away (8 to 90 minutes): the
/// mascot may play a short story. Armed only while one could start
/// (`mascot_timer_wanted`), killed the moment a gate stops holding, and
/// never repeating: a fire re-checks every gate, then the next sync arms a
/// fresh wait. While a story runs the frame clock draws it, not this timer.
/// 19 is the id left free between 18 and 20.
const TIMER_MASCOT_PLAY: usize = 19;
/// One-shot: a lyric line too long for the notch starts scrolling
/// (`LYRIC_HOLD_MS` after it showed); the frame clock then runs until its end.
const TIMER_LYRIC_SCROLL: usize = 16;
/// One-shot: the next System page sample, `SYSTEM_PAGE_MS` after the last
/// one arrived. Only while that page shows (`system_sample_wanted`).
const TIMER_SYSTEM_PAGE: usize = 17;
/// One-shot: the background disk and RAM check, every `SYSTEM_WARN_MS`
/// (sooner to confirm RAM's minute above its limit). It reads memory and
/// drives on the sampler thread and draws nothing unless a notice is raised.
const TIMER_SYSTEM_WARN: usize = 18;
/// Repeating, 40 ms (250 ms after a second without input,
/// `outside_poll_ms`): a mouse button going down outside the island while the
/// panel is kept open (`outside_click_wanted`) closes it. No hook, no capture:
/// the click still goes where it was aimed, and nothing runs while the panel
/// is shut or follows the pointer, or the session is locked or the display off.
const TIMER_OUTSIDE: usize = 24;
const OUTSIDE_MS: u32 = 40;
/// Repeating, `behind::POLL_MS` near the top edge or while lowering is due,
/// `behind::SLOW_MS` otherwise (`behind::poll_ms`): with "Di belakang
/// jendela" on, the pointer is read to raise the island at the top edge and
/// lower it again. Nothing runs with the setting off, in fullscreen, locked
/// or with the display off.
const TIMER_BEHIND: usize = 29;
/// Repeating, `halo::SLOW_MS` (`halo::FAST_MS` while a drag is close,
/// `halo::poll_ms`), but only after a trigger (the pointer moving over the
/// island, a drag entering it) and only while the left button is down or the
/// pointer stays near for `halo::LINGER`: the faint drag margin is drawn while
/// a drag can be about to start (`halo`). An idle island runs no poll; nor
/// does a locked, display-off, fullscreen or behind-windows one.
const TIMER_HALO: usize = 30;
/// Repeating, `pointer_watch::poll_ms`: the one pointer read the closed island
/// shares between the notch peek (it shrinks to the cat's eyes over a window)
/// and the cat following the pointer. 33 ms near the top edge and 250 ms far
/// from it while the cat follows; 80 / 350 ms for the peek alone. Nothing runs
/// when neither feature is eligible, in fullscreen, locked or with the display
/// off; the cat's ease runs on the frame clock, never on this timer.
const TIMER_POINTER: usize = 33;
/// One-shot, re-armed at each step: the typing cat's paws (`TypingPaws::next`
/// gives the wait, 110 to 900 ms) while `typing_anim_on`; one draw per step.
/// Its own id on purpose: `TIMER_BLINK` and `TIMER_ROBOT` already carry the
/// cat's blink, the shrunk eyes' blink and `Brain::next_change`, and sharing
/// one would make each stop or re-arm the others. 40 was the next free id
/// after 35 (`TIMER_UPDATE`) that the other features' notes leave alone.
const TIMER_TYPING: usize = 40;
/// One-shot, `DISPLAYS_MS` after a device-node change (`DBT_DEVNODES_CHANGED`
/// on `WM_DEVICECHANGE`): the monitors are listed again and the island moves
/// only when its screen is not where it was. Re-armed by each change message;
/// never polls. 41 is the next id the other features leave alone.
const TIMER_DISPLAYS: usize = 41;
const DISPLAYS_MS: u32 = 600;
/// Repeating, 1 s: the Mixer page asks the audio thread for the volumes while
/// it shows (`sync_mixer`). Nothing runs with the page shut.
const TIMER_MIXER: usize = 26;
/// One-shot, `lyrics::RETRY_AFTER`: a lookup that failed on the way may be
/// asked again (`LyricsState::retry_due`).
const TIMER_LYRIC_RETRY: usize = 31;
const MIXER_MS: u32 = 1_000;
/// One-shot: the volume plate the wheel brought up goes after this long, and
/// the volume read for it is forgotten (the next wheel reads it again).
const TIMER_VOLUME: usize = 28;
const VOLUME_HUD_MS: u32 = 1_200;
/// How long a start after an update waits for the old island's mutex. The old
/// island frees it itself at the end of its teardown (`release_single_instance`),
/// so this is only the margin for a slow exit (antivirus, Windows Error
/// Reporting keeping a dying process for about 12 s).
const UPDATE_CLAIM_WAIT: Duration = Duration::from_secs(60);
/// The longest the teardown after the message loop may take before the
/// watchdog ends the process anyway.
const TEARDOWN_LIMIT: Duration = Duration::from_secs(5);

/// How this island was started.
pub enum Start {
    /// A plain start (or `--media-log`).
    Normal,
    /// The updater's `--updated <old version>`.
    Updated(String),
    /// `--imported`: Vale restarted itself after importing settings.
    Imported,
}

/// How long a start waits for the single-instance mutex: after an update or an
/// import the old island is still closing; any other start quits at once when
/// another island runs.
fn claim_wait(start: &Start) -> Duration {
    match start {
        Start::Updated(_) | Start::Imported => UPDATE_CLAIM_WAIT,
        Start::Normal => Duration::ZERO,
    }
}
const SYSTEM_PAGE_MS: u32 = crate::system::LIVE_MS;
const SYSTEM_WARN_MS: u32 = 300_000;
const CLI_MS: u32 = 1000;
/// Output lines one wheel notch scrolls (Windows' default of three).
const WHEEL_LINES: i32 = 3;
const SYSTEM_MS: u32 = 1000;
const BATTERY_MS: u32 = 30_000;
/// Values table: ~30 fps equalizer.
const EQ_MS: u32 = 33;
const DAY_MS: u32 = 60_000;
/// The reminder timer re-arms at least this often, so a changed clock or a
/// sleep is noticed without polling.
const REMINDER_MAX_MS: i64 = 3_600_000;
/// A reminder fired this much after its time (sleep, clock change) says "Terlewat".
const REMINDER_LATE_SECS: i64 = 60;
/// Values table: page slide duration (ease-out).
const SLIDE_MS: f32 = 220.0;
/// Values table: the open calendar re-asks for its range once data is this old.
const CALENDAR_FRESH: Duration = Duration::from_secs(60);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// Work that arrived while `APP` was borrowed (a message sent back into the
    /// window procedure mid-handler); done once the outer handler returns.
    static DEFERRED_DRAIN: Cell<bool> = const { Cell::new(false) };
    /// The display on/off notification's handle (`HPOWERNOTIFY`), 0 without one.
    static DISPLAY_NOTIFY: Cell<isize> = const { Cell::new(0) };
    static DEFERRED_REPOSITION: Cell<bool> = const { Cell::new(false) };
    /// The panel header's Settings button asked for the Settings window: it
    /// opens once the handler returns (creating a window while the app is
    /// borrowed would find the app gone).
    static DEFERRED_OPEN_SETTINGS: Cell<bool> = const { Cell::new(false) };
    /// The same for Settings at a card (a notice's "Pengaturan" chip opens
    /// it at Transkrip).
    static DEFERRED_OPEN_CARD: Cell<Option<CardId>> = const { Cell::new(None) };
    /// A transcript window's "Transkrip ulang" asked for its menu (the
    /// recording, the rows, under this button): it opens once the handler
    /// returns, since the dropdown takes the foreground.
    static DEFERRED_TRANSCRIPT_MENU: RefCell<Option<(PathBuf, Vec<crate::ui::menu_pop::Row>, crate::capture::Rect)>> =
        const { RefCell::new(None) };
    /// An update was swapped in and started: the island closes once the
    /// handler returns (`shutdown` destroys the window, which needs the app
    /// unborrowed to run its cleanup).
    static DEFERRED_SHUTDOWN: Cell<bool> = const { Cell::new(false) };
    static DEFERRED_FRAME: Cell<bool> = const { Cell::new(false) };
    static DEFERRED_RESULTS: RefCell<Vec<EditorResult>> = const { RefCell::new(Vec::new()) };
    static DEFERRED_SETTINGS: RefCell<Vec<SettingsAction>> = const { RefCell::new(Vec::new()) };
    /// The screen picker answered while the app was borrowed: its pick is
    /// taken once the handler returns.
    static DEFERRED_SCREEN_PICK: Cell<bool> = const { Cell::new(false) };
    /// The recorded window was minimised (`true`) or restored while the app
    /// was borrowed; the newest word counts.
    static DEFERRED_SCREEN_WATCH: Cell<Option<bool>> = const { Cell::new(None) };
    /// Esc was pressed during the 3-2-1 while the app was borrowed: the
    /// countdown is cancelled once the handler returns.
    static DEFERRED_SCREEN_ESC: Cell<bool> = const { Cell::new(false) };
    static DEFERRED_CLI: RefCell<Vec<InputKey>> = const { RefCell::new(Vec::new()) };
    static DEFERRED_CLI_REDRAW: Cell<bool> = const { Cell::new(false) };
    static DEFERRED_CLIP: RefCell<Vec<InputKey>> = const { RefCell::new(Vec::new()) };
    static DEFERRED_NOTIF: RefCell<Vec<InputKey>> = const { RefCell::new(Vec::new()) };
    static DEFERRED_SEARCH: RefCell<Vec<InputKey>> = const { RefCell::new(Vec::new()) };
    static DEFERRED_ARRANGE: RefCell<Vec<(usize, isize)>> = const { RefCell::new(Vec::new()) };
    /// The island lost the activation (to this window) while the app was
    /// borrowed (inside `SetForegroundWindow`): the lifted notes are told
    /// once the handler returns.
    static DEFERRED_ISLAND_INACTIVE: Cell<Option<isize>> = const { Cell::new(None) };
    /// The tray menu's modal loop is running; a second click must not nest another.
    static MENU_OPEN: Cell<bool> = const { Cell::new(false) };
    /// The single-instance mutexes this process holds (`try_claim`), closed by
    /// `release_single_instance` at the end of the teardown.
    static INSTANCE_MUTEXES: RefCell<Vec<HANDLE>> = const { RefCell::new(Vec::new()) };
    /// The Claude hook pipe's thread, stopped in the teardown before the
    /// mutexes are released (the new island needs the pipe name too).
    static PIPE_SERVER: RefCell<Option<PipeServer>> = const { RefCell::new(None) };
}

/// `media_log_flag`: started with `--media-log`. `start`: how it was started
/// (normal, by the updater, or after an import).
pub fn run(media_log_flag: bool, start: Start) {
    // After an update or an import the old island may still be closing: wait
    // for its mutex.
    if !claim_single_instance(claim_wait(&start)) {
        return;
    }
    // Before anything reads the data folder: DynamicIsland becomes Vale once.
    let moved = data_dir::migrate(&data_dir::root());
    if let data_dir::Migration::Failed(error) = &moved {
        eprintln!("vale: data folder not moved, the old one stays in use: {error}");
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let config = config::load();
    media_log::set_enabled(media_log::requested(media_log_flag, config.media_log));
    i18n::set_lang(config.language);
    crate::ui::tooltip::set_hotkeys(config.hotkeys);
    crate::ui::tooltip::set_font(config.font);
    theme::set_system_light(theme::read_system_light());
    // Before any menu can open: the tray menu follows the Windows app mode.
    tray::follow_system_menu_theme();
    theme::set_current(Theme::of(&config));
    // The screen the notch is on: the saved choice among the displays now
    // (the primary one when it is not connected or there is no choice).
    let displays = arrange_win::displays();
    let target = notch_monitor::resolve(&config.notch_monitor, &displays).map(|i| &displays[i]);
    let placed = target.cloned();
    let win = IslandWindow::create(Some(wnd_proc), target).expect("create island window");
    let mut renderer = Renderer::new().expect("create renderer");
    // Before the first draw: the typeface of the notch and the panel.
    if let Err(error) = renderer.set_ui_font(config.font) {
        eprintln!("vale: cannot set the UI font: {error}");
    }
    let hwnd = win.hwnd();
    let cli_input = CliInput::create(hwnd, deliver_cli_key)
        .inspect_err(|e| eprintln!("vale: no CLI input: {e}"))
        .ok();
    let clip_input = CliInput::create_with(
        hwnd,
        cli_input::ID_CLIP_SEARCH,
        SEARCH_LIMIT,
        deliver_clip_key,
    )
    .inspect_err(|e| eprintln!("vale: no Clipboard search: {e}"))
    .ok();
    let search_input =
        CliInput::create_with(hwnd, cli_input::ID_SEARCH, QUERY_LIMIT, deliver_search_key)
            .inspect_err(|e| eprintln!("vale: no quick search: {e}"))
            .ok();
    // Catches the Notifikasi page's keys only: nothing typed is kept.
    let notif_input = CliInput::create_with(hwnd, cli_input::ID_NOTIF, 8, deliver_notif_key)
        .inspect_err(|e| eprintln!("vale: no Notifikasi input: {e}"))
        .ok();
    let (tx, rx) = mpsc::channel();

    let battery = BatteryMonitor::new(config.battery_threshold);
    let warn = WarnMonitor::new(config.disk_warn_gb, config.ram_warn_percent);
    let _ = tray::set_autostart(config.start_with_windows);
    // Explorer caches the exe's icon (the taskbar button shows it): tell it
    // once per version that it changed.
    app_icon::refresh_shell_icon_once(&app_icon::stamp_path());

    let notch = NotchSize::from_config(&config);
    let rest = shape_sized(config.style, notch, ShapeKind::Idle);
    let rest_fill = notch_fill::fill_alpha(config.notch_fill, config.notch_opacity);
    let pomodoro = config.pomodoro;
    let today = Local::now().date_naive();
    let notes = NotesStore::open(NotesStore::default_path());
    let reminders = RemindersStore::open(RemindersStore::default_path());
    // Sends the cached calendar at once; the first range is asked for below.
    let sync = spawn_sync(tx.clone(), poster(hwnd, WM_APP_EVENT), config.clone());
    // Files dropped on the island go to the shelf.
    shelf_ole::register(hwnd, tx.clone(), poster(hwnd, WM_APP_EVENT));
    // A save finished in the inline capture or the editor: "Disimpan".
    {
        let (tx, wake) = (tx.clone(), poster(hwnd, WM_APP_EVENT));
        crate::shot_edit::set_saved_sink(move |path| {
            if tx.send(Event::ShotFileSaved { path }).is_ok() {
                wake();
            }
        });
    }
    let rec_sources = config.recording.sources;
    APP.with(|cell| {
        *cell.borrow_mut() = Some(App {
            hwnd,
            win: Some(win),
            displays,
            placed,
            renderer,
            config,
            state: IslandState::new(today, pomodoro),
            calendar: CalendarSnapshot::empty(CalStatus::NotConfigured, today),
            notes,
            reminders,
            reminder_fired: Vec::new(),
            sync: Some(sync),
            calendar_asked: None,
            editor: None,
            rx,
            tx: tx.clone(),
            anim: Anim::settled(rest, 0.0),
            slide: None,
            page_fade: None,
            reveal: None,
            strip_motion: None,
            toast_over: false,
            rec_hot_over: None,
            close_fade: CloseFade::settled(0.0, Instant::now()),
            close_hot: false,
            close_ptr: None,
            tip_on: None,
            tip_pressed: None,
            tip_built: None,
            slot_keep: None,
            slot_share: SlotShare::default(),
            leaving: None,
            closed_panel: None,
            fade_floor_h: notch.h,
            fill: (rest_fill, rest_fill),
            animations: client_animations_on(),
            clock: None,
            frame_log: FrameLog::from_env(),
            drawn: View::Hidden,
            fullscreen: false,
            battery,
            system: SystemState::new(warn),
            clean: CleanState::default(),
            mole: MoleState::default(),
            hover: HoverTracking::default(),
            click: ClickPair::default(),
            swipe: gestures::Swipe::default(),
            hwheel: gestures::HWheel::default(),
            media_click: None,
            eq_timer: false,
            draw_gen: 0,
            eq_key: None,
            play: None,
            last_story: None,
            last_play_at: None,
            mascot_timer: false,
            greeting_up: false,
            pending: None,
            clock_hour: Local::now().hour(),
            play_clock_jumped: false,
            eq_capture: None,
            eq_capture_on: false,
            dance_anchor: Instant::now(),
            notice_shown: None,
            notice_serial: 0,
            outside_ms: 0,
            behind: behind::Behind::default(),
            behind_ms: 0,
            rise: None,
            halo_ms: 0,
            peek: Peek::default(),
            under_cache: UnderCache::default(),
            peek_last_pt: None,
            cover_cache: UnderCache::default(),
            eyes: false,
            eyes_morph: false,
            look: LookEase::default(),
            look_at: None,
            eyes_look: EyesLook::default(),
            eyes_look_at: None,
            glance: EyesLook::default(),
            glance_at: None,
            glance_rest: GlanceRest::default(),
            eyes_blink: EyesBlink::default(),
            eyes_blink_timer: false,
            pointer_ms: 0,
            pointer_near: true,
            display_off: false,
            halo_on: false,
            halo_armed: None,
            outside_edge: OutsidePress::default(),
            middle_down: false,
            last_paging: None,
            audio: None,
            mixer: None,
            mixer_pending: false,
            volume_sum: 0,
            volume_cache: None,
            volume_pending: 0,
            volume_hud: None,
            mixer_timer: false,
            mixer_resort: true,
            mixer_seq: 0,
            mixer_set_seq: 0,
            mixer_wheel_sum: None,
            mixer_drag: None,
            shelf: ShelfState::default(),
            phone: PhoneState::default(),
            dock: DockState::default(),
            lyrics_card: LyricsCardState::default(),
            cap: CaptionState::default(),
            devices: DevicesState::default(),
            arrange: ArrangeState::default(),
            hotkeys: HotkeyState::default(),
            widgets: WidgetsState::default(),
            pomo_timer: false,
            rec_timer: false,
            rec_level_timer: false,
            rec: RecState::default(),
            screen: ScreenState::default(),
            tr: TranscribeState::new(),
            rec_sources,
            clock_timer: false,
            progress_timer: false,
            lyrics: LyricsState::default(),
            lyrics_worker: None,
            lyric_timer: false,
            lyric_fading: false,
            lyric_fit: LyricFit::default(),
            lyric_scroll_timer: false,
            lyric_scrolling: false,
            config_warned: false,
            cli: Session::default(),
            cli_run: None,
            cli_runs: 0,
            cli_history: History::load(&cli::history_path()),
            cli_cwd: cli::load_cwd(&cli::cwd_path()),
            cli_seen: 0,
            cli_clock: RunClock::default(),
            cli_echoes: Echoes::default(),
            cli_input,
            cli_prev: None,
            clip: ClipState::default(),
            shots: Shots::default(),
            clip_input,
            notif: NotifState::default(),
            notif_input,
            search: SearchState::default(),
            search_input,
            cli_cursor: HistoryCursor::default(),
            cli_browsing: false,
            cli_pick: 0,
            cli_text: String::new(),
            cli_timer: false,
            cli_wake: Arc::new(AtomicBool::new(false)),
            cli_favorites: FolderList::load(ListKind::Favorites, &cli_folders::favorites_path()),
            cli_recent: FolderList::load(ListKind::Recent, &cli_folders::recent_path()),
            cli_from_explorer: false,
            cli_pinned: false,
            cli_menu_view: FolderMenuView::default(),
            cli_menu_all: Vec::new(),
            cli_menu_scroll: 0,
            explorer: ExplorerReads::default(),
            shortcut: None,
            bata: Bata::new(Instant::now()),
            greet_pending: true,
            greet_ready: false,
            session_locked: false,
            update: UpdateState::default(),
            transfer: Transfer::default(),
        });
    });
    with_app(|app| {
        let now = Instant::now();
        let lyrics_on = app.config.lyrics;
        app.apply(Event::LyricsSetting(lyrics_on), now);
        let clean_mode = app.config.clean_mode;
        app.apply(Event::CleanModeSetting(clean_mode), now);
        let open_mode = app.config.open_mode;
        app.apply(Event::OpenMode(open_mode), now);
        let toast_style = app.config.notifications.toast_style;
        app.apply(Event::ToastStyle(toast_style), now);
        let pages = PanelPages::from_saved(&app.config.panel_pages);
        app.apply(Event::PanelPages(pages), now);
        let tiles = ShortcutTiles::from_saved(&app.config.shortcut_tiles);
        app.apply(Event::ShortcutTiles(tiles), now);
        app.check_fullscreen();
        app.check_battery();
        app.clip_start();
        // The saved history, and the poller when the listener may run.
        app.notif_start();
        app.devices_start();
        app.sync_hotkeys();
        app.widgets_start();
        app.draw(now);
        app.request_calendar(app.state.cal_range(), now);
        // An empty notes list would otherwise look like the notes were deleted.
        if let Some(notice) = notes_unreadable_notice(&app.notes) {
            app.apply_now(Event::Notice(notice));
        }
        if let Some(notice) = reminders_unreadable_notice(&app.reminders) {
            app.apply_now(Event::Notice(notice));
        }
        // Reminders that came due while the island was off ring now.
        app.fire_reminders(true);
        // The sticky notes' too: the ones that came due while it was off.
        app.sticky_remind_fire();
        // The updated notice, the update timer and the leftover cleanup.
        app.update_start(&start);
        app.rec_split_folders();
        app.transcribe_init();
        unsafe {
            SetTimer(Some(hwnd), TIMER_SYSTEM, SYSTEM_MS, None);
            SetTimer(Some(hwnd), TIMER_BATTERY, BATTERY_MS, None);
            SetTimer(Some(hwnd), TIMER_DAY, DAY_MS, None);
            SetTimer(Some(hwnd), TIMER_SYSTEM_WARN, SYSTEM_WARN_MS, None);
            // Unlocking the session may greet; without it, startup and wake still do.
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            // The display going off pauses the pointer polls, like a lock.
            if let Ok(note) = RegisterPowerSettingNotification(
                windows::Win32::Foundation::HANDLE(hwnd.0),
                &GUID_CONSOLE_DISPLAY_STATE,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            ) {
                DISPLAY_NOTIFY.set(note.0);
            }
        }
    });

    let wake = poster(hwnd, WM_APP_EVENT);
    media::spawn_watcher(tx.clone(), wake);
    PIPE_SERVER.with_borrow_mut(|slot| *slot = Some(spawn_pipe_server(tx, wake)));

    // Fails when Explorer's taskbar is not up yet; `TaskbarCreated` retries.
    let _ = tray::add_tray(hwnd);
    request_access_then_onboard(hwnd);

    unsafe {
        let mut msg = MSG::default();
        // 0 is WM_QUIT and -1 an error; both end the loop.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Every way out (tray Quit, the update's shutdown, WM_CLOSE) ends here.
    end_process();
}

/// The teardown after the message loop, then the end of the process. The
/// window is already destroyed (`WM_DESTROY`: stores and history saved, tray
/// icon removed, timers and hotkeys gone).
///
/// ROOT CAUSE (v3.15.0, crash dumps): on every exit the process crashed with
/// 0xc000000d in ntdll, raised on a threadpool thread and called from
/// D3D10Warp.dll during the DLL detach / FLS cleanup in `LdrShutdownProcess`.
/// Our Direct2D render targets are `D2D1_RENDER_TARGET_TYPE_SOFTWARE`, which
/// run on WARP, and they were still alive then: `App` (it owns `Renderer`)
/// sits in the `APP` thread-local, which only the TLS destructors dropped,
/// inside `LdrShutdownProcess`. Windows Error Reporting then kept the dying
/// process, and its `Local\Vale` / `Local\DynamicIsland` mutexes, for about
/// 12 s, while the `--updated` process waited 10 s and quit silently: Vale did
/// not come back after "Update".
///
/// So: drop `App` and every other Direct2D holder explicitly while the process
/// is healthy, stop the pipe, hand over the mutexes ourselves, and then end
/// with `TerminateProcess` instead of returning. The explicit drops are the
/// real fix of our side; `TerminateProcess` is belt and braces, because the
/// crash is inside D3D10Warp's own threadpool cleanup, which runs on the DLL
/// detach even when none of our objects are left. Terminating skips that
/// detach, so an exit can never hang on WER again (the OS closes the handles
/// of a terminated process at once). Child processes are unaffected: the CLI's
/// job object kills them when its handle closes.
///
/// Everything that must reach the disk is written before this runs (config,
/// notes and reminders are saved at each change; the clipboard and
/// notification history in `WM_DESTROY`).
fn end_process() -> ! {
    // A drop that blocks must not keep the island alive (and its mutexes).
    std::thread::spawn(|| {
        std::thread::sleep(TEARDOWN_LIMIT);
        terminate_process();
    });
    // Debug and test builds only: a panic in a destructor must not skip the
    // rest, nor unwind into the normal exit that crashes. The release profile
    // sets `panic = "abort"`, so there `catch_unwind` does nothing: a panicking
    // drop aborts the process (WER may keep the mutexes for about 12 s, and
    // `release_single_instance` and `terminate_process` are skipped). That case
    // is covered by the new process waiting 60 s (`claim_wait`), not by this.
    let _ = std::panic::catch_unwind(|| {
        drop_app();
        release_graphics();
        stop_pipe_server();
        release_single_instance();
    });
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    terminate_process()
}

/// Ends this process at once, without running the DLL detaches.
fn terminate_process() -> ! {
    // SAFETY: the pseudo handle of the current process is always valid.
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
    // Not reached; only in case Windows refuses.
    std::process::exit(0)
}

/// Takes `App` out of `APP` and drops it (its `Renderer`, the CLI inputs, the
/// window, the stores). Outside the borrow, so a destructor that sends a
/// window message finds no app instead of a held borrow.
fn drop_app() {
    let app = APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut slot| slot.take()));
    drop(app);
}

/// Every other holder of Direct2D / DirectWrite objects on the UI thread:
/// each window still open is destroyed and its slot emptied. (`fonts.rs`'s
/// DirectWrite loader is not WARP and may stay.)
fn release_graphics() {
    settings::release();
    editor::release();
    arrange_overlay::release();
    crate::capture::overlay::release();
    crate::shot_edit::clear_saved_sink();
    identify_overlay::release();
    crate::lyrics_card::release();
    crate::caption_card::release();
    crate::shot_edit::window::release();
    crate::transcript_win::release();
}

/// Stops the Claude hook pipe, so the new island can own the name.
fn stop_pipe_server() {
    if let Some(server) = PIPE_SERVER.with_borrow_mut(Option::take) {
        server.stop();
    }
}

/// Posts `msg` to the island from any thread. HWND is not Send, so the
/// closure keeps the raw value and rebuilds it to post.
fn poster(hwnd: HWND, msg: u32) -> impl Fn() + Send + Copy + 'static {
    let raw = hwnd.0 as isize;
    move || unsafe {
        let _ = PostMessageW(Some(HWND(raw as *mut c_void)), msg, WPARAM(0), LPARAM(0));
    }
}

/// False when another island already runs in this session, this name or the
/// one from before the rename (whose data folder must not be moved under
/// it). The mutex handles are held for the life of the process, which also
/// keeps an old Dynamic Island from starting beside it, and closed by the
/// island itself at the end of its teardown (`release_single_instance`), so a
/// start waiting for them (after an update) gets them as soon as it is done.
///
/// `wait`: how long a taken name is retried (every 200 ms) before giving up;
/// zero is a normal start (quit at once), `UPDATE_CLAIM_WAIT` (60 s) is a start
/// after an update, while the old island is still closing.
fn claim_single_instance(wait: Duration) -> bool {
    claim_names(&[w!("Local\\Vale"), w!("Local\\DynamicIsland")], wait)
}

/// `claim_single_instance` for any mutex names.
fn claim_names(names: &[PCWSTR], wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        if try_claim(names) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// One try at every name. `CreateMutexW` returns a handle even when the mutex
/// already exists (`ERROR_ALREADY_EXISTS`), and that handle keeps it alive:
/// after a failed try every handle is closed, or the mutex would outlive its
/// owner and a waiting start would never get it. A successful try keeps its
/// handles in `INSTANCE_MUTEXES` until `release_single_instance`.
fn try_claim(names: &[PCWSTR]) -> bool {
    let mut handles = Vec::new();
    let mut free = true;
    for &name in names {
        // SAFETY: the names are NUL-terminated wide strings.
        match unsafe { CreateMutexW(None, false, name) } {
            Ok(handle) => {
                // Read at once: any other call may change it.
                free &= unsafe { GetLastError() } != ERROR_ALREADY_EXISTS;
                handles.push(handle);
            }
            // Cannot tell; better to run than to show nothing.
            Err(_) => {}
        }
    }
    if free {
        INSTANCE_MUTEXES.with_borrow_mut(|held| held.extend(handles));
    } else {
        for handle in handles {
            let _ = unsafe { CloseHandle(handle) };
        }
    }
    free
}

/// Closes the single-instance mutexes this process holds, so the next island
/// (the `--updated` one, waiting) can claim them at once. Called last in the
/// teardown: the window is destroyed, the data saved, the tray icon removed
/// and the Claude pipe stopped, so nothing of this island is left to clash.
fn release_single_instance() {
    for handle in INSTANCE_MUTEXES.with_borrow_mut(std::mem::take) {
        let _ = unsafe { CloseHandle(handle) };
    }
}

/// Any button down on the island: its tip goes, and the target pressed stays
/// without one until the pointer leaves it (also while the app is held).
fn island_pressed(hwnd: HWND, lparam: LPARAM) {
    tooltip::pressed(hwnd);
    with_app(|app| app.tip_press(lparam));
}

/// The island's tooltip target as the last pointer move gave it: the hit,
/// where it was on screen (px) and the view it was in.
struct IslandTip {
    hit: Hit,
    anchor: crate::capture::Rect,
    view: View,
}

struct App {
    hwnd: HWND,
    /// Taken out only while `reposition` runs, so no borrow is held across it.
    win: Option<IslandWindow>,
    /// The screens as last listed (startup, a display or device change, the
    /// island moving): Settings, Identify and the tray read it.
    displays: Vec<Display>,
    /// The display the window was last placed on (`None`: the primary monitor
    /// by Windows' own answer, no list was available).
    placed: Option<Display>,
    renderer: Renderer,
    config: Config,
    state: IslandState,
    /// The latest calendar data from the sync thread.
    calendar: CalendarSnapshot,
    notes: NotesStore,
    reminders: RemindersStore,
    /// Due reminders whose notice was raised and not yet dismissed; the timer
    /// skips them.
    reminder_fired: Vec<String>,
    /// `None` once shut down.
    sync: Option<SyncHandle>,
    /// The range last asked of the sync thread, and when.
    calendar_asked: Option<((NaiveDate, NaiveDate), Instant)>,
    /// The open note, event or reminder editor (at most one).
    editor: Option<HWND>,
    rx: Receiver<Event>,
    /// Kept to restart the notification poller once access is granted.
    tx: Sender<Event>,
    anim: Anim,
    /// A panel page switch in progress.
    slide: Option<Slide>,
    /// Two pages cross-fading (the search comes or goes over a page).
    page_fade: Option<PageFade>,
    /// A sub-view switch fading in (`ui::motion`): the calendar's range, the
    /// cleaner opening, Mole's stages, the CLI's folder menu.
    reveal: Option<Reveal>,
    /// A busy toast's strip arriving at or leaving the open panel.
    strip_motion: Option<StripAnim>,
    /// The pointer is on the busy toast's own spot, as the reducer last
    /// heard (`Event::ToastPointer`); synced from the state at each refresh.
    toast_over: bool,
    /// The level group of the REC view the pointer is on, as the reducer last
    /// heard (`Event::RecMuteHot`); synced from the state at each refresh.
    rec_hot_over: Option<crate::recorder::Source>,
    /// The notice's ✕ fading in or out (`ui::motion::CloseFade`); its target
    /// is 1 while the pointer is on the island and a closable notice shows
    /// in its full form.
    close_fade: CloseFade,
    /// The pointer is on the ✕ (the lit disc), as the last move saw it.
    close_hot: bool,
    /// Where the pointer last was on the island (`WM_MOUSEMOVE`'s lparam),
    /// `None` once it left: the ✕'s lit disc is re-read from it after a frame
    /// registers the ✕, since a pointer resting where the ✕ fades in sends no
    /// move of its own.
    close_ptr: Option<LPARAM>,
    /// What the last move gave the tooltip (`tip_under`); `None` when it gave
    /// nothing. The tip goes once this no longer holds (`tip_check`).
    tip_on: Option<IslandTip>,
    /// The hit with a tip a button went down on; it stays quiet until the
    /// pointer is on something else (`panel_tips::feed`).
    tip_pressed: Option<Hit>,
    /// The last target built for a move, with the hit, state and language it
    /// was built from: a move that finds them unchanged reuses it.
    tip_built: Option<(Hit, TipCtx, i18n::Lang, tooltip::Target)>,
    /// The toast whose logo the slot shows, kept while the island shrinks
    /// back so the logo fades out instead of vanishing.
    slot_keep: Option<Notice>,
    /// The slot's share of the compact notch's width over the running morph.
    slot_share: SlotShare,
    /// The panel that is closing, as it showed, while its content fades
    /// with the shrinking shape; set at the morph's start, gone at its end.
    leaving: Option<ShownPanel>,
    /// The panel as it showed before the event that took it off screen
    /// (its close resets the cleaner, the CLI's size, the page indices);
    /// `leaving` takes it when the closing morph starts.
    closed_panel: Option<ShownPanel>,
    /// The height at which the panel's content is invisible during the
    /// current morph (`motion::fade_floor`): the target's height while the
    /// panel closes, so its fade ends with the morph.
    fade_floor_h: f32,
    /// The surface fill's alpha the current morph starts from and heads for
    /// (`ui::notch_fill`): 1 for the panel and a solid island, the see-through
    /// alpha for the closed island. At rest both are the same.
    fill: (f32, f32),
    /// Windows' "Animation effects" (`SPI_GETCLIENTAREAANIMATION`) is on.
    /// Off: no slides, no reveals, and the morph eases without overshoot.
    animations: bool,
    /// Paces animation frames; exists only while `anim`, `slide`, `reveal`
    /// or `page_fade` runs, and only while it does.
    clock: Option<FrameClock>,
    frame_log: FrameLog,
    /// The view frames draw: the current one, or the last visible one while
    /// fading out, so the content fades with the shape.
    drawn: View,
    fullscreen: bool,
    battery: BatteryMonitor,
    /// The System page's readings and the disk / RAM warnings.
    system: SystemState,
    /// The cleaner's thread and its last scan.
    clean: CleanState,
    /// The cleaner's Mole mode: its thread, what was found, the log.
    mole: MoleState,
    hover: HoverTracking,
    /// A left press on the island waiting for its release.
    click: ClickPair,
    /// A left-button press that may turn into a swipe, and the sideways
    /// wheel's flicks (`gestures`).
    swipe: gestures::Swipe,
    hwheel: gestures::HWheel,
    /// The transport-button click being reduced, for the diagnosis log; its
    /// `Effect::Media` takes it.
    media_click: Option<media_log::Click>,
    /// `TIMER_EQ` / `TIMER_POMO` are set.
    eq_timer: bool,
    /// Bumped by every message, event and input that reaches the app (every
    /// `with_app` but the equalizer tick's own): an equalizer tick with the
    /// same number as the last drawn frame has nothing new from the app.
    draw_gen: u64,
    /// What the frame on screen drew (`ui::dirty::EqKey`); `None` after a
    /// draw that failed.
    eq_key: Option<EqKey>,
    /// The mascot's story that runs now, and how it started.
    play: Option<ActivePlay>,
    /// The last story played and when it ended: `pick` never repeats it and
    /// the cooldown counts from the time.
    last_story: Option<Story>,
    last_play_at: Option<Instant>,
    /// `TIMER_MASCOT_PLAY` is set.
    mascot_timer: bool,
    /// A greeting notice was on screen at the last refresh.
    greeting_up: bool,
    /// The next fire of `TIMER_MASCOT_PLAY` is this moment's (the greeting's,
    /// or a play picked in the menu), set for when the island's morph ends.
    pending: Option<Moment>,
    /// The hour the idle clock last showed (`TIMER_CLOCK`), to see it roll.
    clock_hour: u32,
    /// The wall clock jumped under a running story (`on_clock_jump`).
    play_clock_jumped: bool,
    /// The loopback capture behind the equalizer bars (`eq_audio`), created
    /// the first time it is wanted; `eq_capture_on` is the last
    /// `set_running` sent to it.
    eq_capture: Option<EqCapture>,
    eq_capture_on: bool,
    /// When `TIMER_EQ` last started: the dancing cat's fallback beat and its
    /// note loop count from here, so a dance always starts at rest.
    dance_anchor: Instant,
    /// The notice view shown at the last `observe`, and a counter bumped when
    /// a different one appears: the mascot's hop and wave start on it.
    notice_shown: Option<Notice>,
    notice_serial: u64,
    /// `TIMER_OUTSIDE`'s interval now; 0 while it does not run.
    outside_ms: u32,
    /// "Di belakang jendela": the island's place in the window order, and
    /// `TIMER_BEHIND`'s interval now (0: not running).
    behind: behind::Behind,
    behind_ms: u32,
    /// The island is coming up from behind a window: drawn dimmer (and a few
    /// dip higher) until it ends. Runs the frame clock only meanwhile.
    rise: Option<behind::Rise>,
    /// `TIMER_HALO`'s interval now (0: not running), and the margin is drawn.
    halo_ms: u32,
    halo_on: bool,
    /// The notch peek (`pointer_watch::Peek`), whether a window lay under the
    /// notch when last read, and whether the island is drawn shrunk to the
    /// eyes (set by `refresh`, so it changes together with the target shape).
    peek: Peek,
    under_cache: UnderCache,
    /// Where the pointer was at the previous peek reading, so a reading can
    /// say whether it moved (`PeekInputs::moved`). Cleared with the peek.
    peek_last_pt: Option<(i32, i32)>,
    /// Whether a window lay over the island when the cat's look last asked,
    /// while stay-behind had it lowered (`look_hidden_behind`).
    cover_cache: UnderCache,
    eyes: bool,
    /// The morph between the eyes and the notch is running: the cat fades
    /// with the rest of the content instead of staying at full strength.
    eyes_morph: bool,
    /// The cat's look at the pointer, and when it was last eased.
    look: LookEase,
    look_at: Option<Instant>,
    /// The shrunk notch's eyes look at the pointer (`pointer_watch::EyesLook`),
    /// and when they were last eased.
    eyes_look: EyesLook,
    eyes_look_at: Option<Instant>,
    /// The typing cat's eyes glance at the pointer (the same pair of eases),
    /// when they were last eased, and the poll readings that tell whether the
    /// pointer still moves (`GlanceRest`).
    glance: EyesLook,
    glance_at: Option<Instant>,
    glance_rest: GlanceRest,
    /// The shrunk notch's blink, and whether `TIMER_BLINK` runs its schedule
    /// (the cat's own blink is `bata.blink_timer`; never both).
    eyes_blink: EyesBlink,
    eyes_blink_timer: bool,
    /// `TIMER_POINTER`'s interval now (0: not running), and whether the
    /// pointer was near the top edge at the last read.
    pointer_ms: u32,
    pointer_near: bool,
    /// The display is off (`GUID_CONSOLE_DISPLAY_STATE`): like a locked
    /// session, the pointer polls pause.
    display_off: bool,
    /// When the last trigger (a move over the island, a drag entering it)
    /// asked for the halo poll; `None` once the poll has stopped.
    halo_armed: Option<Instant>,
    /// The mouse buttons as the last poll saw them, and a press the island
    /// took itself.
    outside_edge: OutsidePress,
    /// The middle button went down on the island while gestures count; its
    /// release there plays or pauses.
    middle_down: bool,
    /// When the wheel last turned a panel page: a paging scroll that passes
    /// over the Music page's cover keeps paging.
    last_paging: Option<Instant>,
    /// The Mixer page's audio thread (started on the first request), the
    /// rows it last read and whether a reading is on its way.
    audio: Option<audio::Audio>,
    mixer: Option<Vec<audio::MixerRow>>,
    mixer_pending: bool,
    /// The wheel over the music: what has not made a percent point yet, the
    /// master level read for it (`TIMER_VOLUME` forgets it), the points
    /// waiting for that reading, and the level the plate shows.
    volume_sum: i32,
    volume_cache: Option<f32>,
    volume_pending: i32,
    volume_hud: Option<f32>,
    /// `TIMER_MIXER` runs.
    mixer_timer: bool,
    /// The page just opened: its first reading sets the row order, which
    /// later readings keep (`audio::keep_order`) until it closes.
    mixer_resort: bool,
    /// Requests to the audio thread are numbered: the last one sent, and
    /// the last level or mute (a reading asked before it is stale).
    mixer_seq: u64,
    mixer_set_seq: u64,
    /// The wheel over a Mixer row: the row and what has not made a notch.
    mixer_wheel_sum: Option<(audio::Target, i32)>,
    /// What the slider the left button is dragging sets, taken at the press
    /// (rows can come and go under a drag). The window holds the capture.
    mixer_drag: Option<audio::Target>,
    /// The file shelf (`app_shelf`).
    shelf: ShelfState,
    /// The phone link (`app_phone`).
    phone: PhoneState,
    /// The page dots' dock: values, pointer, frame step.
    dock: DockState,
    /// The floating lyrics card: open for this run only.
    lyrics_card: LyricsCardState,
    /// Live captions: the session, its worker and its lines (`app_captions`).
    cap: CaptionState,
    /// The Devices page: the drives, their worker and the page's state
    /// (`app_devices`).
    devices: DevicesState,
    /// The Susun page: monitors, the pick step, the mover's results, the
    /// name field and the hotkey (`app_arrange`).
    arrange: ArrangeState,
    /// The five hotkeys in effect, the PrtSc hook and the recording pause (`app_hotkeys`).
    hotkeys: HotkeyState,
    /// The desktop widgets (sticky notes): the host and the pending save.
    widgets: WidgetsState,
    pomo_timer: bool,
    /// `TIMER_REC` is set.
    rec_timer: bool,
    /// `TIMER_REC_LEVEL` is set.
    rec_level_timer: bool,
    /// The audio recorder's session (`app_record`).
    rec: RecState,
    /// The screen recorder's session (`app_screenrec`).
    screen: ScreenState,
    /// Transcript jobs, holds and downloads (`app_transcribe`).
    tr: TranscribeState,
    /// The sources the Rekam page's control chose for the next recording
    /// (kept in `config.recording.sources`).
    rec_sources: RecSources,
    /// `TIMER_CLOCK` is set.
    clock_timer: bool,
    /// `TIMER_PROGRESS` is set.
    progress_timer: bool,
    /// Lyrics looked up, the current track's lines and the line on screen.
    lyrics: LyricsState,
    /// The lyrics thread, started on the first lookup.
    lyrics_worker: Option<LyricsWorker>,
    /// `TIMER_LYRIC` is set.
    lyric_timer: bool,
    /// Two lyric lines are crossfading; the frame clock runs until it ends.
    lyric_fading: bool,
    /// The lyric notch's width and its line's scroll.
    lyric_fit: LyricFit,
    /// `TIMER_LYRIC_SCROLL` is set.
    lyric_scroll_timer: bool,
    /// A long lyric line is scrolling; the frame clock runs until it ends.
    lyric_scrolling: bool,
    /// The invalid-config notice was shown this run.
    config_warned: bool,
    /// The CLI's output, last command and running state.
    cli: Session,
    /// The running command, if any.
    cli_run: Option<RunHandle>,
    /// Run ids given out so far.
    cli_runs: u64,
    cli_history: History,
    /// Where commands run; `cd` changes it.
    cli_cwd: PathBuf,
    /// `Output::total` as of the last drain: what is new since.
    cli_seen: u64,
    /// When the running or last command started and how it ended.
    cli_clock: RunClock,
    /// Which output lines are echoed commands.
    cli_echoes: Echoes,
    /// The hidden EDIT behind the CLI page's input line.
    cli_input: Option<CliInput>,
    /// The foreground window before the input (the CLI's or the Clipboard
    /// search) took the keyboard; it gets it back on Esc, a page change or
    /// the panel closing.
    cli_prev: Option<HWND>,
    /// The clipboard history and the Clipboard page's own state.
    clip: ClipState,
    /// The screenshot preview's screenshots (`app_shot`).
    shots: Shots,
    /// The hidden EDIT behind the Clipboard page's search field.
    clip_input: Option<CliInput>,
    /// The Notifikasi page: its history and state, and the hidden EDIT that
    /// catches its keys.
    notif: NotifState,
    notif_input: Option<CliInput>,
    /// The quick search's own state, and the hidden EDIT behind its field.
    search: SearchState,
    search_input: Option<CliInput>,
    /// Up / Down through the history while no suggestions show.
    cli_cursor: HistoryCursor,
    /// The input holds a history entry put there by Up / Down: no
    /// suggestions until it is edited.
    cli_browsing: bool,
    /// The highlighted suggestion.
    cli_pick: usize,
    /// The input's text as last seen, to tell the user's edits from ours.
    cli_text: String,
    /// `TIMER_CLI` is set.
    cli_timer: bool,
    /// Set by a run's threads when they post a wake, cleared when `drain`
    /// reads the queue: a burst of output posts one message, not hundreds.
    cli_wake: Arc<AtomicBool>,
    /// The folder menu's starred and recently used folders.
    cli_favorites: FolderList,
    cli_recent: FolderList,
    /// `cli_cwd` is the last active Explorer window's folder, followed as
    /// the page opened: the "dari Explorer" chip shows.
    cli_from_explorer: bool,
    /// A `cd` or a menu pick chose the folder during this panel visit: the
    /// page does not follow Explorer again until the panel closes.
    cli_pinned: bool,
    /// The open folder menu's lines, built when it opens or its lists change.
    cli_menu_view: FolderMenuView,
    /// Every line of the folder menu; the view holds them scrolled.
    cli_menu_all: Vec<cli_folders::MenuItem>,
    /// Lines of the folder menu scrolled away by the wheel.
    cli_menu_scroll: usize,
    /// Explorer folders read off the UI thread, and what waits for them.
    explorer: ExplorerReads,
    /// A clicked shortcut waiting for `TIMER_SHORTCUT`.
    shortcut: Option<shortcuts::Action>,
    /// Bata, the robot: its state, blink and timers.
    bata: Bata,
    /// The startup greeting waits for the first calendar data (the cache,
    /// sent at once), so it can count today's events.
    greet_pending: bool,
    /// That data arrived: greet after this drain.
    greet_ready: bool,
    /// The last lock or unlock `WM_WTSSESSION_CHANGE` said locked; used when
    /// asking Windows for the session's state fails.
    session_locked: bool,
    /// The self-update: the known release, a check or a download running.
    update: UpdateState,
    /// "Ekspor / Impor pengaturan": the card's state and the file waiting to be confirmed.
    transfer: Transfer,
}

/// The File Explorer reader (`sources::explorer`): at most one read runs.
#[derive(Default)]
struct ExplorerReads {
    /// The last list read, the last active window first; the menu shows it
    /// until a fresher one arrives.
    folders: Vec<PathBuf>,
    /// Reads asked for so far (request ids).
    asked: u64,
    /// The read running, if any.
    running: Option<u64>,
    /// The read started because the CLI page opened: its result may move
    /// the CLI there.
    for_open: Option<u64>,
    /// Another read asked for while one ran (true: for the page opening).
    queued: Option<bool>,
    /// When the running read started.
    started: Option<Instant>,
}

/// A read of the Explorer windows still out after this is given up on (a
/// hung Explorer): the next one may start.
const EXPLORER_READ_TIMEOUT: Duration = Duration::from_secs(3);

impl ExplorerReads {
    /// A read runs and is not yet given up on.
    fn busy(&self, now: Instant) -> bool {
        self.running.is_some()
            && self
                .started
                .is_some_and(|at| now.duration_since(at) < EXPLORER_READ_TIMEOUT)
    }
}

/// The System page and the warnings: the sampler thread (started on the
/// first request), what it last read, and which one-shots are set.
struct SystemState {
    sampler: Option<Sampler>,
    /// The last page sample; the page draws dashes without one.
    sample: Option<SystemSample>,
    history: CpuHistory,
    /// The page's figures, easing toward each reading.
    ease: SysEase,
    /// Readings asked for since the page showed: the first and every fourth
    /// walks the processes, the others are cheap (`system::kind_for_request`).
    requests: u32,
    monitor: WarnMonitor,
    /// A page sample was asked for and has not come back.
    page_pending: bool,
    /// `TIMER_SYSTEM_PAGE` is set.
    page_timer: bool,
    /// A warning check was asked for and has not come back.
    warn_pending: bool,
    /// A reading raised a notice during this drain: the island refreshes.
    raised: bool,
}

impl SystemState {
    fn new(monitor: WarnMonitor) -> Self {
        Self {
            sampler: None,
            sample: None,
            history: CpuHistory::default(),
            ease: SysEase::default(),
            requests: 0,
            monitor,
            page_pending: false,
            page_timer: false,
            warn_pending: false,
            raised: false,
        }
    }

    /// The last reading is older than `HISTORY_STALE` (the page was closed
    /// since): it goes, with its line and its easing, so the page shows
    /// dashes and the next reading shows at once instead of gliding from it.
    fn forget_stale(&mut self, now: Instant) -> bool {
        let stale = self.history.stale(now);
        if stale {
            self.sample = None;
            self.history = CpuHistory::default();
            self.ease = SysEase::default();
        }
        stale
    }
}

/// The cleaner: its thread (started on the first scan), the last scan and
/// whether one runs. Nothing here runs or redraws while the cleaner is not
/// open; a clean already asked for finishes in the background.
#[derive(Default)]
struct CleanState {
    worker: Option<Cleaner>,
    scan: Option<ScanResult>,
    scanning: bool,
}

/// The Mole mode (`crate::mole`): its thread (started on first use), the
/// Mole found, the last finished preview, the run whose output counts and
/// its log. Nothing runs or redraws while the cleaner is closed, except a
/// confirmed clean, which goes on.
#[derive(Default)]
struct MoleState {
    worker: Option<MoleWorker>,
    install: Option<mole::Install>,
    /// The last dry-run that finished well, for the options it ran with.
    preview: Option<mole::Preview>,
    /// The run whose lines and result count; older ones are ignored.
    run: u64,
    /// Newest last, at most `mole::LOG_CAP` lines.
    log: VecDeque<String>,
    /// The last clean's measured result.
    report: Option<mole::MoleReport>,
}

/// What the island keeps for the robot besides its `Brain`.
struct Bata {
    brain: Brain,
    rng: Rng,
    /// The eyes are shut (between the blink timer's two halves).
    blinking: bool,
    /// `TIMER_BLINK` / `TIMER_ROBOT` are set.
    blink_timer: bool,
    change_timer: bool,
    /// Windows' Energy Saver is on: Full motion runs as Saver.
    energy_saver: bool,
    /// The typing cat's paws, and whether `TIMER_TYPING` is armed for them.
    paws: TypingPaws,
    typing_timer: bool,
}

impl Bata {
    fn new(now: Instant) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            brain: Brain::new(now),
            rng: Rng::new(seed),
            blinking: false,
            blink_timer: false,
            change_timer: false,
            energy_saver: false,
            paws: TypingPaws::default(),
            typing_timer: false,
        }
    }
}

impl App {
    fn drain(&mut self) {
        self.draw_gen = self.draw_gen.wrapping_add(1);
        let now = Instant::now();
        // Before reading: output queued after this posts a new wake.
        self.cli_wake.store(false, Ordering::SeqCst);
        let (mut received, mut samples) = (0usize, 0usize);
        while let Ok(event) = self.rx.try_recv() {
            received += 1;
            // Background readings draw what they changed themselves.
            // A toast redraws what it changed itself and raises its notice
            // (`system.raised`) unless a muted app's.
            samples += usize::from(matches!(
                event,
                Event::SystemSample(_)
                    | Event::Audio(..)
                    | Event::Devices(_)
                    | Event::Arrange(_)
                    | Event::Toast(_)
                    | Event::ClipPacked { .. }
                    | Event::Update(_)
                    | Event::Rec(..)
                    | Event::Screen(..)
                    | Event::Transcribe(..)
                    | Event::TranscribeDownload(..)
                    // The caption card follows by itself (`captions_flush`).
                    | Event::Captions(..)
                    // A progress tick draws the receiving view itself, at
                    // most six a second; nothing else changed.
                    | Event::Phone(_, crate::phone::PhoneEvent::UploadProgress { .. })
                    // A phone that joined redraws the phone view itself (who
                    // is connected); a file found gone redraws the shelf.
                    // The island has nothing new to show.
                    | Event::Phone(_, crate::phone::PhoneEvent::Joined { .. })
                    | Event::Phone(_, crate::phone::PhoneEvent::SharedGone { .. })
            ));
            self.apply(event, now);
        }
        // Once per burst of command output, not per chunk.
        self.note_cli_output(now);
        // An open transcript window follows its live job once per burst too.
        self.transcript_flush_live();
        // The caption card follows its worker once per burst as well.
        self.captions_flush();
        // Only system readings: they drew what they changed (the open
        // System page); a background check must not redraw the island,
        // unless it raised a notice.
        let raised = std::mem::take(&mut self.system.raised);
        if drain_refreshes(received, samples, raised) {
            self.refresh(now);
        }
        if std::mem::take(&mut self.greet_ready) {
            self.maybe_greet(GreetTrigger::Startup);
        }
    }

    /// Tells the reducer how many output lines arrived since last time, so a
    /// view scrolled up stays on what it shows.
    fn note_cli_output(&mut self, now: Instant) {
        let total = self.cli.output.total();
        let new = total.saturating_sub(self.cli_seen) as usize;
        self.cli_seen = total;
        if new > 0 {
            self.apply(Event::CliOutput(new), now);
        }
    }

    /// Runs `event` through the reducer and performs its effects; events an
    /// effect produces (an editor opened, a notice) follow in order.
    fn apply(&mut self, event: Event, now: Instant) {
        self.draw_gen = self.draw_gen.wrapping_add(1);
        let mut queue = VecDeque::from([event]);
        while let Some(event) = queue.pop_front() {
            self.robot_hears(&event, now);
            let event = match event {
                // A session without metadata yet has nothing to show.
                Event::Media(Some(m)) if m.title.is_empty() => Event::Media(None),
                // Only `App` keeps the calendar; the reducer has no use for it.
                Event::Calendar(snapshot) => {
                    self.calendar = snapshot;
                    if std::mem::take(&mut self.greet_pending) {
                        self.greet_ready = true;
                    }
                    continue;
                }
                // Lyrics are `App`'s too; `refresh` tells the reducer they exist.
                Event::Lyrics(found) => {
                    let retry = self.lyrics.found(found);
                    // The card says "not found" or shows the lines now.
                    self.sync_lyrics_card(now);
                    if retry {
                        // A failed lookup: asked again after the back-off.
                        unsafe {
                            SetTimer(
                                Some(self.hwnd),
                                TIMER_LYRIC_RETRY,
                                lyrics::RETRY_AFTER.as_millis() as u32,
                                None,
                            );
                        }
                    }
                    continue;
                }
                Event::Cli(event) => {
                    self.on_cli_event(event);
                    continue;
                }
                Event::ExplorerFolders { request, folders } => {
                    self.on_explorer_folders(request, folders);
                    continue;
                }
                Event::SystemSample(sample) => {
                    self.on_system_sample(sample, now);
                    continue;
                }
                Event::Audio(asked, rows) => {
                    self.on_audio(asked, rows);
                    continue;
                }
                Event::Devices(event) => {
                    self.on_devices_event(event, now);
                    continue;
                }
                Event::Arrange(event) => {
                    self.on_arrange_event(event, now);
                    continue;
                }
                // The notch notice and the history entry are decided here.
                Event::Toast(toast) => {
                    self.on_toast(toast, now);
                    continue;
                }
                Event::ClipPacked { id, fingerprint, packed } => {
                    self.on_clip_packed(id, fingerprint, packed);
                    continue;
                }
                // The self-update's workers: `App` decides what shows.
                Event::Update(event) => {
                    self.on_update_event(event);
                    continue;
                }
                // The recorder's thread: `App` makes the notice.
                Event::Rec(session, note) => {
                    self.on_rec_note(session, note);
                    continue;
                }
                // The screen recorder's threads, the same way.
                Event::Screen(session, note) => {
                    self.on_screen_note(session, note);
                    continue;
                }
                // A transcript job's and a model download's threads: `App`
                // decides what shows.
                Event::Transcribe(id, event) => {
                    self.on_transcribe(id, event);
                    continue;
                }
                Event::TranscribeDownload(id, event) => {
                    self.on_transcribe_download(id, event);
                    continue;
                }
                // A caption worker's thread, the same way.
                Event::Captions(id, event) => {
                    self.on_captions(id, event);
                    continue;
                }
                // The scan knows only what is on disk; the queue's marks go over it.
                Event::RecRecent(list, total) => {
                    self.rec.recent = list.clone();
                    self.rec.total = total;
                    Event::RecRecent(self.transcribe_marks(list), total)
                }
                Event::VideoRecent(list, total) => {
                    self.screen.videos = list.clone();
                    self.screen.total = total;
                    Event::VideoRecent(self.transcribe_marks(list), total)
                }
                Event::SearchIndex(apps) => {
                    self.on_search_index(apps);
                    continue;
                }
                Event::SearchFiles(files) => {
                    self.on_search_files(files);
                    continue;
                }
                Event::SearchDetail {
                    generation,
                    key,
                    detail,
                } => {
                    self.on_search_detail(generation, key, detail);
                    continue;
                }
                Event::ShelfDragOver(on) => {
                    // A drag that entered the island is a halo trigger: if
                    // it strays above the island once the panel has gone,
                    // the margin can catch it (the refresh starts the poll).
                    if on {
                        self.halo_armed = Some(Instant::now());
                    }
                    match self.on_shelf_drag(on) {
                        Some(event) => event,
                        None => continue,
                    }
                }
                Event::ShelfDrop(paths) => {
                    self.on_shelf_drop(paths, now);
                    continue;
                }
                // The phone link's server: `App` keeps what it said and
                // tells the reducer only the stage.
                Event::Phone(session, event) => {
                    self.on_phone_event(session, event, now);
                    continue;
                }
                Event::Clean(event) => {
                    self.on_clean_event(event, now);
                    continue;
                }
                Event::Mole(event) => {
                    self.on_mole_event(event, now);
                    continue;
                }
                Event::ShotFolder { shot, dir, found } => {
                    self.on_shot_folder(shot, dir, found);
                    continue;
                }
                Event::ShotSaveDone { path, written, .. } => {
                    self.on_shot_save_done(path, written, now);
                    continue;
                }
                Event::ShotFileSaved { path } => {
                    self.on_shot_file_saved(path, now);
                    continue;
                }
                Event::ShotRecycled { file, emptied, .. } => {
                    self.on_shot_recycled(file, emptied, now);
                    continue;
                }
                other => other,
            };
            let page = self.state.page();
            let before = self.panel_snap();
            let strip_before = self.strip_snap();
            let strip_notice = self
                .state
                .waiting_strip()
                .filter(|n| is_real_toast(n))
                .cloned();
            let shown_before = self.state.shown_panel();
            let cli_was = (
                self.state.view() == View::Panel(Page::Cli),
                self.state.cli_menu(),
                self.state.panel_is_open(),
            );
            self.media_click = match &event {
                Event::Click(hit) if media_log::enabled() && is_media_hit(hit) => {
                    Some(media_log::Click {
                        hit: hit.clone(),
                        page,
                        hover_open_ms: self
                            .state
                            .hover_open_age(now)
                            .map(|age| age.as_millis() as u64),
                        guarded: self.state.media_guarded(now),
                    })
                }
                _ => None,
            };
            for effect in self.state.apply(event, now) {
                queue.extend(self.perform(effect, now));
            }
            // No command came of it: the hover guard dropped it, or there
            // was nothing to play or pause.
            if let Some(click) = self.media_click.take() {
                let why = if click.guarded { "guard" } else { "no media" };
                media_log::log_command(&click, media_log::Outcome::Ignored(why), &self.media_app());
            }
            self.follow_transition(before, now);
            self.follow_strip(strip_before, strip_notice, now);
            self.follow_closed_panel(shown_before);
            self.follow_cli_page(cli_was);
            self.follow_clip_page(cli_was.2);
            self.follow_notif_page(cli_was.2);
            self.follow_shot();
        }
    }

    /// After the reducer: the CLI page just showed (read Explorer for its
    /// start folder), its folder menu just opened (read again, so its
    /// windows are fresh, and list what is known meanwhile), or the panel
    /// closed (the next visit may follow Explorer again).
    fn follow_cli_page(&mut self, (was_cli, was_menu, was_open): (bool, bool, bool)) {
        if was_open && !self.state.panel_is_open() {
            self.cli_pinned = false;
        }
        let shown = self.state.view() == View::Panel(Page::Cli);
        // Only when following could happen: not in "Folder terakhir" mode,
        // nor after a cd this visit.
        if shown
            && !was_cli
            && self.config.cli_start == cli_folders::CliStart::Explorer
            && !self.cli_pinned
        {
            self.read_explorer(true);
        }
        // "Folder terakhir" set after following Explorer: back to the
        // user's own folder (no read needed).
        if shown && !was_cli && self.config.cli_start == cli_folders::CliStart::Last {
            self.back_to_last_cli_folder();
        }
        if self.state.cli_menu() && !was_menu {
            self.cli_menu_scroll = 0;
            self.rebuild_cli_menu();
            self.read_explorer(false);
        }
    }

    /// Starts a read of the Explorer windows on a background thread, or
    /// queues it behind the one running.
    fn read_explorer(&mut self, for_open: bool) {
        let reads = &mut self.explorer;
        let now = Instant::now();
        if reads.busy(now) {
            reads.queued = Some(reads.queued.unwrap_or(false) || for_open);
            return;
        }
        reads.asked += 1;
        let id = reads.asked;
        if explorer::read_folders(id, self.tx.clone(), poster(self.hwnd, WM_APP_EVENT)) {
            reads.running = Some(id);
            reads.started = Some(now);
            if for_open {
                reads.for_open = Some(id);
            }
        }
    }

    /// A read finished: the menu lists its windows; the read for the page
    /// opening may move the CLI to the last active one.
    fn on_explorer_folders(&mut self, request: u64, folders: Vec<PathBuf>) {
        // A read given up on that answers after a newer one started: stale.
        if self.explorer.running.is_some_and(|r| request < r) {
            return;
        }
        if self.explorer.running == Some(request) {
            self.explorer.running = None;
        }
        self.explorer.folders = folders;
        if self.explorer.for_open == Some(request) {
            self.explorer.for_open = None;
            self.follow_explorer();
        }
        if self.state.cli_menu() {
            self.rebuild_cli_menu();
        }
        if let Some(for_open) = self.explorer.queued.take() {
            self.read_explorer(for_open);
        }
    }

    /// Moves the CLI to the last active Explorer folder if `choose_on_open`
    /// says so, checked now (a command may have started since the read
    /// began). An automatic move does not write cli-cwd.txt: that file
    /// stays the user's own last folder for "Folder terakhir".
    fn follow_explorer(&mut self) {
        if self.state.view() != View::Panel(Page::Cli) {
            return;
        }
        let Some(dir) = cli_folders::choose_on_open(
            self.config.cli_start,
            self.cli.running().is_some(),
            self.cli_pinned,
            &self.explorer.folders,
            &self.cli_cwd,
        ) else {
            // No Explorer window any more: not its folder either.
            self.back_to_last_cli_folder();
            return;
        };
        let line = i18n::cli_moved_to(i18n::lang(), &dir.to_string_lossy());
        self.cli_cwd = dir;
        self.cli_from_explorer = true;
        self.cli.note(line);
    }

    /// A folder taken from Explorer gives way to the user's own last one
    /// (cli-cwd.txt) when `back_to_last` says so, with the same "moved" line.
    fn back_to_last_cli_folder(&mut self) {
        if !cli_folders::back_to_last(
            self.config.cli_start,
            self.cli.running().is_some(),
            self.cli_pinned,
            &self.explorer.folders,
            self.cli_from_explorer,
        ) {
            return;
        }
        let dir = cli::load_cwd(&cli::cwd_path());
        self.cli_from_explorer = false;
        if dir != self.cli_cwd {
            self.cli
                .note(i18n::cli_back_to(i18n::lang(), &dir.to_string_lossy()));
            self.cli_cwd = dir;
        }
    }

    /// The menu's lines from the lists as they are now; gone folders hidden.
    fn rebuild_cli_menu(&mut self) {
        let sections = cli_folders::menu_rows(
            &self.explorer.folders,
            &self.cli_favorites,
            &self.cli_recent,
            Local::now().naive_local(),
            // Never a network check on the UI thread.
            &cli_folders::exists_quick,
        );
        self.cli_menu_all = cli_folders::menu_items(&sections);
        self.cli_menu_scroll = self
            .cli_menu_scroll
            .min(cli_folders::max_scroll(&self.cli_menu_all));
        self.cli_menu_view.items = cli_folders::scrolled(&self.cli_menu_all, self.cli_menu_scroll);
    }

    /// The user chose `dir` (`cd` or the menu): it is saved as the last
    /// folder and the newest recent one, and it holds until the panel
    /// closes.
    fn set_cli_folder(&mut self, dir: PathBuf) {
        let _ = cli::save_cwd(&cli::cwd_path(), &dir);
        self.remember_recent(&dir);
        self.cli_cwd = dir;
        self.cli_pinned = true;
        self.cli_from_explorer = false;
    }

    fn remember_recent(&mut self, dir: &std::path::Path) {
        self.cli_recent.push_recent(dir, Local::now().timestamp());
        // A list that cannot be written only loses the list.
        let _ = self.cli_recent.save(&cli_folders::recent_path());
    }

    /// A folder picked in the menu; never while a command runs.
    fn pick_cli_folder(&mut self, dir: PathBuf) {
        if self.cli.running().is_some() {
            return;
        }
        if cli_folders::exists_quick(&dir) {
            self.set_cli_folder(dir);
        } else {
            self.cli.note(cli::cd_error(&dir.to_string_lossy()));
        }
        self.note_cli_output(Instant::now());
    }

    /// Stars or unstars `dir`; the open menu follows.
    fn set_cli_favorite(&mut self, dir: &std::path::Path, on: bool) {
        if self.cli_favorites.set_favorite(dir, on) {
            let _ = self.cli_favorites.save(&cli_folders::favorites_path());
            if self.state.cli_menu() {
                self.rebuild_cli_menu();
            }
            self.redraw_cli();
        } else if on && self.cli_favorites.is_full() {
            // A star that did nothing says why.
            self.cli.note(i18n::cli_favorites_full(i18n::lang()));
            self.redraw_cli();
        }
    }

    /// A right-click on a favourite in the open menu unstars it, one on a
    /// Notes card asks for the card menu, one on a Rekam row asks for its
    /// transcript menu, anywhere else the tray menu opens as always.
    fn on_right_click(&mut self, lparam: LPARAM) -> RightClick {
        // The faint drag margin is not the island: no menu from it.
        let hit = match self.under_at(lparam) {
            Under::Island(hit) => Some(hit),
            Under::Halo | Under::Off => return RightClick::Done,
        };
        if self.clip_right_click(hit.as_ref()) {
            return RightClick::Done;
        }
        if let Some(id) = note_card_menu_target(self.state.view(), hit.as_ref()) {
            return RightClick::NoteMenu(id);
        }
        if let Some(i) = rec_row_menu_target(self.state.view(), hit.as_ref()) {
            return RightClick::RecRowMenu(i);
        }
        if !self.state.cli_menu() || self.state.view() != View::Panel(Page::Cli) {
            return RightClick::Tray;
        }
        match menu_right_click(self.hit_at(lparam)) {
            MenuRightClick::Unstar(path) => {
                self.set_cli_favorite(&path, false);
                RightClick::Done
            }
            MenuRightClick::Swallow => RightClick::Done,
            MenuRightClick::Tray => RightClick::Tray,
        }
    }

    /// The AppUserModelId of the media session on show; empty without one.
    fn media_app(&self) -> String {
        self.state
            .media()
            .map(|m| m.source_app.clone())
            .unwrap_or_default()
    }

    /// Bata's side of an event: activity resets its sleep clock, Claude
    /// sessions start or stop working.
    fn robot_hears(&mut self, event: &Event, now: Instant) {
        let brain = &mut self.bata.brain;
        if is_activity(event) {
            brain.activity(now);
        }
        match event {
            Event::ClaudeActivity { session_id } => brain.claude_activity(session_id, now),
            Event::Notice(n) => match &n.kind {
                NoticeKind::ClaudeDone { session_id }
                | NoticeKind::ClaudeWaiting { session_id } => brain.claude_ended(session_id),
                _ => {}
            },
            _ => {}
        }
    }

    /// The robot's motion policy in effect (Energy Saver turns Full into Saver).
    fn motion(&self) -> Motion {
        self.config
            .robot_animation
            .effective(self.bata.energy_saver)
    }

    fn sleep_after(&self) -> Duration {
        Duration::from_secs(u64::from(robot::clamp_sleep(self.config.robot_sleep_minutes)) * 60)
    }

    /// Bata's face and pose in `view` at `now`; `None` when it is off or the
    /// view has none.
    fn robot_frame(&self, view: &View, now: Instant) -> Option<RobotFrame> {
        if !self.config.robot {
            return None;
        }
        let motion = self.motion();
        let spot = Spot::of(view);
        let inputs = self
            .bata
            .brain
            .inputs(spot, now, self.sleep_after(), motion);
        let expr = choose_expr(&inputs)?;
        let mut pose = self
            .bata
            .brain
            .pose(
                spot,
                now,
                motion.moves(),
                self.bata.blinking,
                self.config.mascot,
            );
        // The idle cat turns toward the pointer; nowhere else, and only with
        // a face that looks (nothing floating beside its head).
        if spot == Spot::Idle && self.look.value() != 0.0 {
            pose = pose.looking_as(expr, self.look.value());
        }
        // The typing cat only glances (eyes, never the head) and its paws
        // take turns on `TIMER_TYPING`. With motion not Full `Brain::pose`
        // is still: paws down, eyes on the screen.
        if spot == Spot::Idle && expr.glances() && motion.moves() {
            let (vx, vy) = self.glance.value();
            pose = pose.glancing(vx, vy);
            if self.bata.typing_timer {
                pose.paw_lift = self.bata.paws.lift();
            }
        }
        // The music cat nods to the bass (or a fixed beat when capture is not
        // live) and its note floats up and fades, on the `TIMER_EQ` ticks.
        let playing = self.state.media().is_some_and(|m| m.playing);
        if robot::dance_on(
            self.config.robot,
            self.config.cat_dance,
            motion,
            self.animations,
            playing,
            self.eq_timer,
            expr,
        ) {
            let since = now.saturating_duration_since(self.dance_anchor);
            let amount = robot::nod_amount(
                self.eq_bass()
                    .unwrap_or_else(|| robot::fallback_beat(since)),
            );
            let (rise, fade) = robot::note_phase(since);
            pose = pose.dance(amount, rise, fade);
            pose.beat = std::f32::consts::PI * since.as_millis() as f32 / robot::BEAT_MS as f32;
        }
        Some(RobotFrame { expr, pose })
    }

    /// A lift or bounce of a robot on screen is moving: the frame clock runs
    /// for it. No robot drawn (panel, hidden, an app's notice): nothing moves.
    fn robot_moving(&self, now: Instant) -> bool {
        let view = self.state.view();
        self.robot_frame(&view, now).is_some()
            && self
                .bata
                .brain
                .moving(
                    Spot::of(&view),
                    now,
                    self.motion().moves(),
                    self.config.mascot,
                )
    }

    /// The cat's sleep clock takes the last mouse or keyboard input anywhere
    /// on the PC (one `GetLastInputInfo`). A failed read changes nothing, so
    /// it cannot keep the cat awake.
    fn fold_pc_input(&mut self, now: Instant) {
        if !self.config.robot {
            return;
        }
        if let Some(ms) = input_idle() {
            self.bata
                .brain
                .input_seen(now, Duration::from_millis(u64::from(ms)));
        }
    }

    /// Arms or stops Bata's one-shot timers for what shows now.
    fn sync_robot_timers(&mut self, now: Instant) {
        let face = self.robot_frame(&self.state.view(), now).map(|r| r.expr);
        let mut plan = plan_timers(face, self.motion(), self.animating_at(now));
        // The shrunk notch has no cat to blink: its eyes run their own
        // schedule below, on the same timer.
        plan.blink &= !self.eyes;
        if plan.blink != self.bata.blink_timer {
            self.bata.blink_timer = plan.blink;
            self.bata.blinking = false;
            unsafe {
                if plan.blink {
                    let wait = robot::blink_delay_ms(&mut self.bata.rng);
                    SetTimer(Some(self.hwnd), TIMER_BLINK, wait, None);
                } else {
                    let _ = KillTimer(Some(self.hwnd), TIMER_BLINK);
                }
            }
        }
        self.sync_eyes_blink();
        // With Motion Off, the typing cat's wider island still shrinks back
        // when a Claude session goes quiet (`robot::change_at`).
        let typing_shape = matches!(self.state.view(), View::Idle) && self.typing_shape(now);
        let next = robot::change_at(
            &self.bata.brain,
            plan,
            typing_shape,
            now,
            self.sleep_after(),
        );
        // A sleeping cat on screen also looks for PC input every 1.8 s, as
        // one-shots only; nothing polls while it is awake.
        let wake = robot::wake_poll_on(
            face,
            Spot::of(&self.state.view()),
            self.motion(),
            self.eyes,
            self.fullscreen,
            self.screen_away(),
        );
        let next = robot::robot_timer_at(next, wake, now);
        unsafe {
            match next {
                // The wake poll is coalescable; its slack keeps it within 2 s.
                // `SetCoalescableTimer` on the same id replaces the pending one.
                Some((_, true)) => {
                    SetCoalescableTimer(
                        Some(self.hwnd),
                        TIMER_ROBOT,
                        robot::WAKE_POLL_MS,
                        None,
                        robot::WAKE_POLL_SLACK_MS,
                    );
                }
                // `SetTimer` on the same id replaces the pending one.
                Some((at, false)) => {
                    let wait = at.saturating_duration_since(now) + Duration::from_millis(1);
                    let ms = wait.as_millis().min(u32::MAX as u128) as u32;
                    SetTimer(Some(self.hwnd), TIMER_ROBOT, ms, None);
                }
                None if self.bata.change_timer => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_ROBOT);
                }
                None => {}
            }
        }
        self.bata.change_timer = next.is_some();
        self.sync_typing_timer(now);
        self.sync_mascot_timer(now);
    }

    /// `TIMER_BLINK`: shut the eyes for `BLINK_MS`, then open them and wait
    /// 4–7 s for the next blink.
    fn on_blink(&mut self) {
        if self.eyes_blink_timer {
            // The shrunk notch's eyes: one step of their schedule. A running
            // ease does not postpone it: the clock draws the shut state, and
            // otherwise this draws it.
            let wait = self.eyes_blink.next(&mut self.bata.rng);
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_BLINK, wait, None);
            }
            if !self.animating() {
                self.draw(Instant::now());
            }
            return;
        }
        if !self.bata.blink_timer {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_BLINK);
            }
            return;
        }
        self.bata.blinking = !self.bata.blinking;
        let wait = if self.bata.blinking {
            robot::BLINK_MS
        } else {
            robot::blink_delay_ms(&mut self.bata.rng)
        };
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_BLINK, wait, None);
        }
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// The shrunk notch's eyes may blink: it is shrunk (with or without the
    /// cat), Motion is Full (not Energy Saver) and Windows animations are on.
    /// Not held back by the frame clock: the eyes' ease would postpone every
    /// blink.
    fn eyes_blink_wanted(&self) -> bool {
        self.eyes && self.motion().moves() && self.animations
    }

    /// Arms or kills `TIMER_BLINK` for the eyes' blink schedule, and starts
    /// the schedule open. Whichever of the cat's and the eyes' schedule
    /// stops leaves the shared timer to the other: the cat's block of
    /// `sync_robot_timers` has already armed or killed it by its own plan.
    fn sync_eyes_blink(&mut self) {
        let wanted = self.eyes_blink_wanted();
        if wanted == self.eyes_blink_timer {
            return;
        }
        self.eyes_blink_timer = wanted;
        self.eyes_blink = EyesBlink::default();
        unsafe {
            if wanted {
                let wait = EyesBlink::open_wait(&mut self.bata.rng);
                SetTimer(Some(self.hwnd), TIMER_BLINK, wait, None);
            } else if !self.bata.blink_timer {
                let _ = KillTimer(Some(self.hwnd), TIMER_BLINK);
            }
        }
    }

    /// The typing cat's paws should move now (`robot::typing_anim_on`) at
    /// `now`: the face is Typing and motion, Windows' animations, fullscreen,
    /// the lock or display state and the shrunk notch allow it.
    fn typing_anim_wanted(&self, now: Instant) -> bool {
        let face = self.robot_frame(&self.state.view(), now).map(|r| r.expr);
        robot::typing_anim_on(
            face,
            self.motion(),
            self.animations,
            self.fullscreen,
            self.screen_away(),
            self.eyes,
        )
    }

    /// Arms or kills `TIMER_TYPING`: a one-shot, re-armed with each step's
    /// wait (`TypingPaws::next`) while the paws should move, killed
    /// otherwise with both paws down (drawn once if one was up and the typing
    /// cat is still on screen). No frame clock, no repeating timer.
    fn sync_typing_timer(&mut self, now: Instant) {
        let wanted = self.typing_anim_wanted(now);
        if wanted == self.bata.typing_timer {
            return;
        }
        self.bata.typing_timer = wanted;
        if wanted {
            let wait = self.bata.paws.next(&mut self.bata.rng);
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_TYPING, wait, None);
            }
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TYPING);
        }
        let was_up = self.bata.paws.up().is_some();
        self.bata.paws.reset();
        let typing_shows = self
            .robot_frame(&self.state.view(), now)
            .is_some_and(|r| r.expr == Expr::Typing);
        if was_up && typing_shows && !self.animating_at(now) {
            self.draw(now);
        }
    }

    /// `TIMER_TYPING`: one step of the paws, the next one armed, one draw
    /// (unless the frame clock draws anyway). A tick that is no longer wanted
    /// ends the timer instead.
    fn on_typing_timer(&mut self) {
        let now = Instant::now();
        if !self.bata.typing_timer {
            // A tick posted just before the timer was killed.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_TYPING);
            }
            return;
        }
        if !self.typing_anim_wanted(now) {
            // Kills the timer and puts the paws down.
            self.sync_typing_timer(now);
            return;
        }
        let wait = self.bata.paws.next(&mut self.bata.rng);
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_TYPING, wait, None);
        }
        if !self.animating_at(now) {
            self.draw(now);
        }
    }

    /// What the play gates read now. With `read_pointer`, the pointer is read
    /// (one `GetCursorPos`) unless the pointer poll runs and already holds it:
    /// with both pointer features off `pointer_near` is never refreshed.
    fn play_env(&self, now: Instant, read_pointer: bool) -> Env {
        let view = self.state.view();
        let asleep = self
            .robot_frame(&view, now)
            .is_some_and(|r| r.expr == Expr::Sleep);
        Env {
            mascot_on: self.config.robot,
            motion_full: self.config.robot_animation == Motion::Full,
            energy_saver: self.bata.energy_saver,
            win_anim: self.animations,
            idle_view: view == View::Idle,
            panel_open: matches!(view, View::Panel(_)),
            menu_open: MENU_OPEN.with(Cell::get),
            fullscreen: self.fullscreen,
            screen_away: self.screen_away(),
            shrunk: self.eyes,
            pointer_near: read_pointer && self.pointer_near_now(),
            asleep,
            recording: hotkeys::any_recording(self.rec.active(), self.screen.recording()),
            clock_on: self.config.idle_clock,
            has_story: has_ready(self.config.mascot, self.config.idle_clock),
            working: self.typing_shape(now),
        }
    }

    /// The pointer is near the island's top edge. Unknown counts as near.
    fn pointer_near_now(&self) -> bool {
        if self.pointer_ms != 0 {
            return self.pointer_near;
        }
        let mut pt = POINT::default();
        self.near_top_at(unsafe { GetCursorPos(&mut pt) }.is_ok().then_some(pt))
    }

    /// `pointer_near_now` for a cursor already read. Unknown counts as near.
    fn near_top_at(&self, cursor: Option<POINT>) -> bool {
        match (self.win.as_ref(), self.notch_span(), cursor) {
            (Some(win), Some(span), Some(pt)) => {
                pointer_watch::near_top(pt.x, pt.y - win.screen_origin().1, span, win.scale())
            }
            _ => true,
        }
    }

    /// Arms or kills `TIMER_MASCOT_PLAY`: a one-shot while a story could
    /// start, killed the moment that stops holding. The pointer is not read
    /// here (`pointer_near` may be stale with the pointer poll off) and, like
    /// the presence rule and the cooldown, is judged when it fires.
    fn sync_mascot_timer(&mut self, now: Instant) {
        // The timer is a pending moment's for now; its fire checks every gate.
        if self.pending.is_some() {
            return;
        }
        let env = self.play_env(now, false);
        let preset = self.config.mascot_play;
        let wanted = mascot_timer_wanted(&env, preset, self.play.is_some());
        let Some(arm) = timer_change(self.mascot_timer, wanted) else {
            return;
        };
        self.mascot_timer = arm;
        unsafe {
            let wait = wait_for(preset, self.bata.rng.next_u32());
            match wait {
                Some(wait) if arm => {
                    let ms = wait.as_millis().min(u128::from(u32::MAX)) as u32;
                    SetTimer(Some(self.hwnd), TIMER_MASCOT_PLAY, ms, None);
                }
                _ => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_MASCOT_PLAY);
                    self.mascot_timer = false;
                }
            }
        }
    }

    /// `TIMER_MASCOT_PLAY` fired: gone unless re-armed. Every gate is checked
    /// again; a story starts only if all still hold, and the next sync arms
    /// a new wait only if one could start.
    fn on_mascot_play_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_MASCOT_PLAY);
        }
        self.mascot_timer = false;
        let now = Instant::now();
        if let Some(moment) = std::mem::take(&mut self.pending) {
            self.play_when_settled(moment, now);
        } else {
            self.try_play(Moment::Idle, now);
        }
        self.sync_mascot_timer(now);
    }

    /// `moment`'s story starts once the island has reached its idle shape (the
    /// greeting notice ended, or the panel was shut for a menu pick). While
    /// the morph runs, `TIMER_MASCOT_PLAY` is set for its remaining time and
    /// marked as `moment`'s (no new timer). Every gate is checked when it
    /// starts.
    fn play_when_settled(&mut self, moment: Moment, now: Instant) {
        let left = morph_left(self.anim.running, self.anim.start, self.anim.ms, now);
        let env = Env {
            pointer_near: false,
            ..self.play_env(now, false)
        };
        let could = self.play.is_none()
            && may_start(
                &env,
                moment,
                self.config.mascot_play,
                Duration::ZERO,
                since_last_play(self.last_play_at, now),
            );
        match settle_plan(left, could) {
            Settle::Now => self.try_play(moment, now),
            Settle::Never => {}
            Settle::Wait(wait) => {
                self.pending = Some(moment);
                self.mascot_timer = true;
                let ms = wait.as_millis() as u32;
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_MASCOT_PLAY, ms, None);
                }
            }
        }
    }

    /// "Mainkan maskot": shuts an open panel, then plays around the settled
    /// island.
    fn on_play_mascot_menu(&mut self) {
        if matches!(self.state.view(), View::Panel(_)) {
            self.apply_now(Event::PanelShut);
        }
        self.play_when_settled(Moment::Manual, Instant::now());
    }

    /// Starts a story for `moment` if every gate allows it right now.
    fn try_play(&mut self, moment: Moment, now: Instant) {
        if self.play.is_some() {
            return;
        }
        // One `GetLastInputInfo`, on the rare moments a story is considered.
        self.fold_pc_input(now);
        let env = self.play_env(now, true);
        let idle = self.bata.brain.since_activity(now);
        if may_start(
            &env,
            moment,
            self.config.mascot_play,
            idle,
            since_last_play(self.last_play_at, now),
        ) {
            self.start_play(moment, now);
        }
    }

    /// Picks a story and starts it: the frame clock runs until it is done.
    fn start_play(&mut self, moment: Moment, now: Instant) {
        let story = pick(
            self.config.mascot,
            self.config.idle_clock,
            self.last_story,
            moment,
            self.bata.rng.next_u32(),
        );
        self.play = Some(ActivePlay {
            play: Play::start(story, now),
            moment,
            shape: play_shape_of(self.config.style, self.notch_size(), self.config.mascot),
        });
        self.play_clock_jumped = false;
        self.ensure_clock();
        // The timer that may have started it is spent.
        self.sync_mascot_timer(now);
    }

    /// One frame of a running story, before it is drawn: ends it (at once, or
    /// easing home) when a gate stops holding, and drops it when it is done.
    fn advance_play(&mut self, now: Instant) {
        if self.play.is_none() {
            return;
        }
        // The one pointer reading of this frame feeds the presence gate and
        // the click-through test.
        let mut pt = POINT::default();
        let cursor = unsafe { GetCursorPos(&mut pt) }.is_ok().then_some(pt);
        let near = if self.pointer_ms != 0 {
            self.pointer_near
        } else {
            self.near_top_at(cursor)
        };
        let env = Env {
            pointer_near: near,
            ..self.play_env(now, false)
        };
        let shape = play_shape_of(self.config.style, self.notch_size(), self.config.mascot);
        let jumped = std::mem::take(&mut self.play_clock_jumped);
        let inside = self.cursor_on_island(cursor, now);
        let buttons = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .into_iter()
            .any(|vk| (unsafe { GetAsyncKeyState(i32::from(vk.0)) }) as u16 & 0x8000 != 0);
        let out = advance_slot(&mut self.play, &env, shape, jumped, buttons, inside, now);
        if let Some(story) = out.finished {
            self.last_story = Some(story);
            self.last_play_at = Some(now);
        }
        if let Some(win) = self.win.as_mut() {
            win.set_click_through(out.click_through);
        }
    }

    /// The cursor is on the island's resting shape. Unknown counts as on it,
    /// so the island stays clickable.
    fn cursor_on_island(&self, cursor: Option<POINT>, now: Instant) -> bool {
        let (Some(pt), Some(win)) = (cursor, self.win.as_ref()) else {
            return true;
        };
        let (origin, k) = (win.screen_origin(), win.scale());
        let (x, y) = ((pt.x - origin.0) as f32 / k, (pt.y - origin.1) as f32 / k);
        swallows(self.config.style, self.idle_shape(now), true, x, y)
    }

    /// What the renderer gets for the running story on the island `shape`:
    /// the story's shot at this beat (at rest for a story with no timeline).
    fn play_frame(&self, now: Instant, shape: ShapeRect) -> Option<PlayFrame> {
        self.play.as_ref().map(|active| {
            let beat = active.play.beat(now);
            let shot = stories::shot(active.play.story(), self.config.style, shape, beat);
            PlayFrame::of(shot, beat)
        })
    }

    /// The hour changed since the idle clock last showed: the hour moment.
    fn check_hour(&mut self, now: Instant) {
        let hour = Local::now().hour();
        if hour_rolled(self.clock_hour, hour) {
            self.clock_hour = hour;
            self.try_play(Moment::Hour, now);
        }
    }

    /// The shrunk notch's eyes this frame: where they look, and shut for a
    /// blink. Centred and open anywhere else.
    fn eyes_pose(&self) -> EyesPose {
        if !self.eyes {
            return EyesPose::default();
        }
        let (dx_dip, dy_dip) = self.eyes_look.offset_dip();
        EyesPose {
            dx_dip,
            dy_dip,
            blink: self.eyes_blink.shut(),
        }
    }

    /// The session is locked now: Windows' answer, or the last lock/unlock
    /// message when it gives none.
    fn session_locked(&self) -> bool {
        query_session_locked().unwrap_or(self.session_locked)
    }

    /// Once per period of the day (`greeting`), at startup, a wake by the
    /// user or unlock: the cat's greeting for that period with the counts
    /// that fit it, for 4 s. Not on a locked session (`greet_may_try`) nor
    /// over a fullscreen app: then it waits for the next chance. The day and
    /// period are saved when it shows.
    fn maybe_greet(&mut self, trigger: GreetTrigger) {
        if !greet_may_try(trigger, self.session_locked()) {
            return;
        }
        let now = Local::now().naive_local();
        let settings = GreetSettings {
            robot: self.config.robot,
            greeting: self.config.morning_greeting,
        };
        let Some(period) = should_greet(self.config.last_greeting, now, settings) else {
            return;
        };
        if self.state.fullscreen() {
            return;
        }
        let counts = greeting_counts(
            &self.calendar.events,
            self.reminders.upcoming(),
            now,
            period,
        );
        let lang = i18n::lang();
        let notice = Notice {
            key: "bata-greeting".into(),
            source: tr(Key::SourceVale).into(),
            title: i18n::greeting_title(lang, period).into(),
            body: i18n::greeting_body(lang, period, counts),
            kind: NoticeKind::Greeting { period },
        };
        self.apply_now(Event::Notice(notice));
        self.config.last_greeting = Some(GreetMark {
            date: now.date(),
            period,
        });
        self.save_config();
    }

    fn perform(&mut self, effect: Effect, now: Instant) -> Option<Event> {
        match effect {
            Effect::OpenTarget(target) => notif_open::open(target),
            Effect::OpenClaudeSession { session_id } => notif_open::open_claude(session_id),
            // The saved screenshot's notice was clicked: Explorer opens on
            // its folder with the file selected. The island just got the
            // click, so it may let that window take the foreground; the rest
            // runs on a worker.
            Effect::RevealFile(file) => {
                notif_open::allow_foreground();
                crate::sources::reveal::reveal(file);
            }
            // To the session the island shows (by key), not whichever is current.
            Effect::Media(cmd) => {
                let target = self.state.media().map_or_else(
                    || media::Target {
                        key: 0,
                        app: String::new(),
                    },
                    |m| media::Target {
                        key: m.session_key,
                        app: m.source_app.clone(),
                    },
                );
                media::send_command(cmd, target, self.media_click.take())
            }
            Effect::PlaySound => play_phase_sound(),
            Effect::SavePomodoro(settings) => {
                self.config.pomodoro = settings;
                self.save_config();
            }
            Effect::ConnectGoogle => return self.connect_google(),
            Effect::RefreshCalendar { from, to } => self.request_calendar((from, to), now),
            Effect::OpenNoteEditor(id) => {
                return self.open_editor(note_editor_kind(&self.notes, id.as_deref()));
            }
            Effect::ToggleNoteSticky(id) => self.sticky_toggle(&id),
            Effect::OpenEventEditor { event_id, date } => {
                let kind = event_editor_kind(&self.calendar.events, event_id.as_deref(), date)?;
                return self.open_editor(kind);
            }
            Effect::OpenReminderEditor(id) => {
                let now = Local::now().naive_local();
                return self.open_editor(reminder_editor_kind(&self.reminders, id.as_deref(), now));
            }
            Effect::SaveLyrics(on) => {
                self.config.lyrics = on;
                self.save_config();
                if !on {
                    self.lyrics_card_lyrics_off();
                }
            }
            Effect::ToggleLyricsCard => self.toggle_lyrics_card(),
            // The Clipboard tile has no action: the reducer opened its page.
            Effect::RunShortcut(s) => self.queue_shortcut(shortcuts::shortcut_action(s)?),
            Effect::RunCustom(tile) => self.queue_shortcut(shortcuts::Action::Custom(tile)),
            Effect::RunCapture => {
                crate::ui::tooltip::hide_all();
                self.queue_shortcut(shortcuts::Action::Capture);
            }
            Effect::OpenDrive(letter) => self.queue_shortcut(shortcuts::Action::Drive(letter)),
            Effect::MixerLevel(i, level) => self.mixer_set_level(i, level),
            Effect::MixerMute(i) => self.mixer_toggle_mute(i),
            Effect::ShelfOpen(i) => self.shelf_open(i),
            Effect::ShelfRemove(i) => self.shelf_remove(i),
            Effect::ShelfClear => self.shelf_clear(),
            Effect::PhoneStart => return self.phone_pill(),
            Effect::PhoneNewQr => self.phone_new_qr(),
            Effect::PhoneNet(n) => self.phone_switch(n),
            Effect::PhoneCancel => self.phone_cancel(),
            Effect::PhoneStop => self.phone_stop(crate::phone::EndReason::Done),
            Effect::PhoneBack => self.phone_back(),
            Effect::RecToggle => self.rec_toggle(),
            Effect::CaptionToggle => self.captions_toggle(),
            Effect::RecPause => self.rec_pause(),
            Effect::RecMute(source) => self.rec_toggle_mute(source),
            Effect::RecDevice(source) => self.rec_open_device_menu(source),
            Effect::RecQualityMenu => self.rec_open_quality_menu(),
            Effect::RecSources(sources) => self.rec_set_sources(sources),
            Effect::RecReveal(path) => self.rec_reveal(path),
            Effect::RecOpenFolder => self.rec_open_folder(),
            Effect::RecMicSettings => self.rec_mic_settings(),
            Effect::RecPickFolder => self.rec_pick_folder(),
            Effect::TranscribeQueue(audio) => self.transcribe_queue(audio, None),
            Effect::OpenTranscript(audio) => self.open_transcript(audio),
            Effect::OpenTranscriptSettings => self.open_transcript_settings(),
            Effect::OpenWhatsNew => DEFERRED_OPEN_CARD.set(Some(CardId::WhatsNew)),
            Effect::RecPickFile(kind) => self.rec_pick_file(kind),
            Effect::ScreenToggle => self.screen_toggle(),
            Effect::ScreenDiscard => self.screen_discard(),
            Effect::VideoOpenFolder => self.screen_open_folder(),
            Effect::TranscriptOff => self.transcript_off_note(),
            Effect::DeviceEject(i) => self.devices_eject(i),
            Effect::DeviceOpen(i) => self.devices_open(i),
            Effect::DeviceDetail(i) => self.devices_toggle_detail(i),
            Effect::DevicesEjectAll => self.devices_eject_all(),
            Effect::ArrangeMonitor(i) => self.arrange_pick_monitor(i),
            Effect::ArrangeCard(i) => self.arrange_card(i),
            Effect::ArrangePlay(i) => self.arrange_play(i),
            Effect::ArrangeWindow(i) => self.arrange_window(i),
            Effect::ArrangeSkip => self.arrange_skip(),
            Effect::ArrangeUndo => self.arrange_undo(),
            Effect::ArrangeSave => self.arrange_save(),
            Effect::OpenCleaner => return self.scan_clean(),
            Effect::CleanDelete(cats) => return self.start_clean(cats),
            Effect::SaveCleanMode(mode) => {
                self.config.clean_mode = mode;
                self.save_config();
            }
            Effect::MoleDetect => return self.mole_detect(),
            Effect::MolePreview(opts) => return self.mole_preview(opts),
            Effect::MoleStopPreview => self.mole_stop_preview(),
            Effect::MoleClean(opts) => return self.mole_clean(opts),
            Effect::MoleWhitelist => {
                if let Some(install) = &self.mole.install
                    && let Err(error) = mole::open_whitelist(install)
                {
                    eprintln!("vale: cannot open Mole's whitelist: {error}");
                }
            }
            Effect::OpenMoleInstall => {
                self.queue_shortcut(shortcuts::Action::Open(mole::README_URL));
            }
            Effect::FocusCliInput => return self.focus_cli_input(),
            Effect::ReleaseCliFocus => self.return_keyboard(),
            Effect::StopCli => self.stop_cli(),
            Effect::CliChangeFolder(dir) => self.pick_cli_folder(dir),
            Effect::CliFavorite { path, on } => self.set_cli_favorite(&path, on),
            Effect::FocusClipSearch => return self.focus_clip_search(),
            Effect::ReleaseClipFocus => self.return_keyboard(),
            Effect::FocusNotifInput => return self.focus_notif_input(),
            Effect::ReleaseNotifFocus => self.return_keyboard(),
            Effect::Notif(action) => self.perform_notif(action),
            Effect::FocusSearch => return self.focus_search(),
            Effect::ReleaseSearchFocus => self.release_search(),
            Effect::SearchRun(i) => return self.run_search(i),
            Effect::Clip(action) => return self.perform_clip(action),
            Effect::Shot(action) => return self.perform_shot(action),
            Effect::ReminderDone(id) => {
                // The reducer already took its notice down.
                self.reminder_fired.retain(|f| *f != id);
                if self
                    .reminders
                    .mark_done_or_advance(&id, Local::now().timestamp())
                {
                    self.persist_reminders(now);
                }
                self.arm_reminders();
            }
            Effect::StickySnooze(id) => self.sticky_snooze(&id),
            Effect::StickyOpen(id) => self.sticky_open(&id),
            Effect::OpenSettings => DEFERRED_OPEN_SETTINGS.set(true),
            Effect::InstallUpdate => self.update_install(),
            Effect::OpenReleases => self.update_open_releases(),
        }
        None
    }

    /// Raises a notice (and one sound) for every reminder whose time has come
    /// and that is not showing yet, then re-arms the timer. At startup they all
    /// say "Terlewat"; later only those fired well after their time (sleep).
    fn fire_reminders(&mut self, startup: bool) {
        let now = Instant::now();
        let unix = Local::now().timestamp();
        let mut rang = false;
        for id in due_now(self.reminders.list(), unix) {
            if self.reminder_fired.contains(&id) {
                continue;
            }
            let Some(notice) = self
                .reminders
                .get(&id)
                .map(|r| reminder_notice(r, unix, startup))
            else {
                continue;
            };
            self.reminder_fired.push(id);
            self.apply(Event::Notice(notice), now);
            rang = true;
        }
        if rang {
            play_phase_sound();
            self.refresh(now);
        }
        self.arm_reminders();
    }

    /// One-shot to the earliest reminder not showing yet, capped at
    /// `REMINDER_MAX_MS` so a clock change or sleep is caught within the hour.
    fn arm_reminders(&self) {
        let pending: Vec<Reminder> = self
            .reminders
            .upcoming()
            .iter()
            .filter(|r| !self.reminder_fired.contains(&r.id))
            .cloned()
            .collect();
        let wait = reminder_wait_ms(next_due(&pending), Local::now().timestamp_millis());
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_REMINDER);
            if let Some(ms) = wait {
                SetTimer(Some(self.hwnd), TIMER_REMINDER, ms, None);
            }
        }
    }

    /// After an edit or delete: notices of reminders that are no longer due
    /// come down, and the timer follows the new times.
    fn settle_reminders(&mut self, now: Instant) {
        let due = due_now(self.reminders.list(), Local::now().timestamp());
        let (keep, gone): (Vec<String>, Vec<String>) = std::mem::take(&mut self.reminder_fired)
            .into_iter()
            .partition(|id| due.contains(id));
        self.reminder_fired = keep;
        for id in gone {
            self.apply(Event::ReminderCleared(id), now);
        }
        self.arm_reminders();
    }

    fn persist_reminders(&mut self, now: Instant) {
        if let Err(error) = self.reminders.save() {
            let notice = island_notice(
                "reminders",
                tr(Key::RemindersSaveFailed),
                &error.to_string(),
            );
            self.apply(Event::Notice(notice), now);
        }
    }

    /// What the panel shows, for `follow_transition`.
    fn panel_snap(&self) -> Snap {
        let s = &self.state;
        let cleaner = s.cleaner();
        Snap {
            panel: matches!(s.view(), View::Panel(_)),
            page: s.page(),
            sub: SubView {
                cal_view: s.cal_view(),
                cal_anchor: s.cal_anchor(),
                day_page: s.day_page(),
                notes_page: s.notes_page(),
                reminders_page: s.reminders_page(),
                cleaner: cleaner.map(|c| c.mode),
                mole: mole_code(s.mole_phase()),
                mole_log: cleaner.is_some_and(|c| c.mole_log),
                cli_menu: s.cli_menu(),
                phone: s.phone_stage(),
            },
        }
    }

    /// After any applied event: what changed inside the open panel moves
    /// (`ui::motion::transition`). A page change slides (the wheel, the
    /// Clipboard tile, a disk warning's "Buka", a page switched off while it
    /// shows), the search cross-fades, a sub-view switch (the calendar's
    /// range, the cleaner, Mole's stages, the CLI menu) reveals. A slide that
    /// starts during one continues from where the page is. Nothing runs
    /// unless something changed, and everything rides the frame clock.
    fn follow_transition(&mut self, before: Snap, now: Instant) {
        let after = self.panel_snap();
        match transition(&before, &after, self.animations) {
            Transition::None => {
                if !after.panel {
                    self.slide = None;
                    self.page_fade = None;
                    self.reveal = None;
                }
            }
            Transition::Slide => {
                // A later page in the owner's order comes in from the right.
                let dir = slide_from(self.state.pages(), before.page, after.page);
                // One that runs goes on as the same strip: the pages on
                // screen keep their places.
                let running = self.slide.filter(|s| s.progress(now) < 1.0);
                let offset = running.map_or(0.0, |s| s.offset(now));
                let strip = running.map(|s| s.strip);
                self.slide = Some(Slide {
                    strip: SlideStrip::next(strip.as_ref(), offset, before.page, dir),
                    start: now,
                });
                self.page_fade = None;
                self.reveal = None;
                self.ensure_clock();
            }
            Transition::Fade => {
                self.page_fade = Some(PageFade {
                    from_page: before.page,
                    start: now,
                });
                self.slide = None;
                self.reveal = None;
                self.ensure_clock();
            }
            Transition::Reveal(dir) => {
                // One cut short continues from the opacity it had.
                let from_alpha = self
                    .reveal
                    .filter(|r| !r.done(now))
                    .map_or(0.0, |r| r.frame(now).alpha);
                self.reveal = Some(Reveal::new(now, dir, from_alpha));
                self.ensure_clock();
            }
        }
    }

    /// What the panel's strip holds, for `follow_strip`.
    fn strip_snap(&self) -> StripSnap {
        StripSnap {
            panel: matches!(self.state.view(), View::Panel(_)),
            page: self.state.page(),
            strip: self.state.waiting_strip().map(is_real_toast),
        }
    }

    /// After any applied event: a busy toast's strip that came to the open
    /// panel or left it moves (`ui::motion::strip_dir`: the content eases
    /// down or up and the strip fades, `REVEAL_MS`); a toast and a sticky
    /// strip swapping is instant. `leaving` is the toast's notice, for the
    /// strip that fades out. Rides the frame clock like the reveal.
    fn follow_strip(&mut self, before: StripSnap, leaving: Option<Notice>, now: Instant) {
        let after = self.strip_snap();
        match strip_dir(before, after, !self.anim.running, self.animations) {
            Some(dir) => {
                self.strip_motion = Some(StripAnim {
                    motion: StripMotion { start: now, dir },
                    leaving: leaving.filter(|_| dir == StripDir::Out),
                });
                self.ensure_clock();
            }
            None => {
                // One that no longer fits what shows (the panel closed or
                // turned a page; a strip came back while one left) ends.
                let stale = match self.strip_motion.as_ref().map(|m| m.motion.dir) {
                    Some(StripDir::In) => after.strip.is_none(),
                    Some(StripDir::Out) => after.strip.is_some(),
                    None => false,
                };
                if !after.panel || before.page != after.page || stale {
                    self.strip_motion = None;
                }
            }
        }
    }

    /// After any applied event: the panel that showed before it and does not
    /// now is kept as it was, for its fade-out (`refresh` hands it to
    /// `leaving`). Its close already reset what it showed.
    fn follow_closed_panel(&mut self, shown_before: Option<ShownPanel>) {
        self.closed_panel =
            closed_panel_after(&self.state.view(), shown_before, self.closed_panel.take());
    }

    /// Without a client ID there is nothing to log in to; the notice says
    /// where to set it. config.json is read again first, so an ID added while
    /// the island runs is used without a restart. A second click while the
    /// browser login waits is ignored.
    fn connect_google(&mut self) -> Option<Event> {
        let file_ok = self.reload_config();
        match connect_action(file_ok, client::for_config(&self.config)) {
            // The ID may well be in there; say so rather than "fill it in".
            ConnectAction::InvalidConfig => Some(Event::Notice(invalid_config_notice(None))),
            ConnectAction::NoClientId => Some(Event::Notice(client_id_notice())),
            ConnectAction::Login => {
                if let Some(sync) = &self.sync {
                    sync.connect();
                }
                None
            }
        }
    }

    /// Asks the sync thread for `range` unless `needs_fetch` says the data is fresh.
    fn request_calendar(&mut self, range: (NaiveDate, NaiveDate), now: Instant) {
        let snap = &self.calendar;
        let fetched_age = snap
            .fetched_at
            .filter(|_| snap.status == CalStatus::Ok && (snap.from, snap.to) == range)
            .and_then(|at| (Local::now() - at).to_std().ok());
        if !needs_fetch(self.calendar_asked, fetched_age, range, now) {
            return;
        }
        let (from, to) = range;
        if self
            .sync
            .as_ref()
            .is_some_and(|sync| sync.send(SyncRequest::Range { from, to }))
        {
            self.calendar_asked = Some((range, now));
        }
    }

    /// Opens an editor under the panel. If one is already open, Windows
    /// brings that one forward instead and nothing else changes.
    fn open_editor(&mut self, kind: EditorKind) -> Option<Event> {
        let anchor = self.under_panel()?;
        match editor::open_editor(self.hwnd, anchor, kind) {
            Ok(hwnd) if self.editor == Some(hwnd) => None,
            Ok(hwnd) => {
                self.editor = Some(hwnd);
                // Keeps the panel open while the pointer is over the editor.
                Some(Event::EditorOpen(true))
            }
            Err(error) => {
                eprintln!("vale: cannot open the editor: {error}");
                None
            }
        }
    }

    /// Screen point under the panel's bottom centre, where editors hang.
    fn under_panel(&self) -> Option<POINT> {
        let win = self.win.as_ref()?;
        let panel = shape_for(self.config.style, self.panel_kind());
        Some(win.to_screen(panel.x + panel.w / 2.0, panel.y + panel.h))
    }

    /// The open panel's shape: enlarged on the CLI page when asked for,
    /// taller for the cleaner.
    fn panel_kind(&self) -> ShapeKind {
        let cleaner = self.state.cleaner().is_some();
        panel_kind(self.state.page(), self.state.cli_tall(), cleaner)
    }

    /// The cleaner thread, started on first use; `None` when it cannot run.
    fn cleaner(&mut self) -> Option<&Cleaner> {
        if self.clean.worker.is_none() {
            self.clean.worker = Cleaner::spawn(self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        }
        self.clean.worker.as_ref()
    }

    /// The cleaner opened (or a clean ended while it shows): measure every
    /// category. Not while a clean runs; its end scans again. A scan the
    /// thread cannot take tells the reducer, so the next showing asks again.
    fn scan_clean(&mut self) -> Option<Event> {
        if self.state.clean_busy() || self.clean.scanning {
            return None;
        }
        let sent = self.cleaner().is_some_and(Cleaner::scan);
        self.clean.scanning = sent;
        if sent {
            return None;
        }
        self.clean.worker = None;
        Some(Event::CleanScanFailed)
    }

    /// The confirmed clean. If the thread cannot take it, the rows take
    /// clicks again at once.
    fn start_clean(&mut self, cats: Vec<CleanCat>) -> Option<Event> {
        if self.cleaner().is_some_and(|c| c.delete(cats)) {
            return None;
        }
        self.clean.worker = None;
        Some(Event::CleanFinished)
    }

    /// A scan or a clean came back. A clean raises its notice (Bata happy),
    /// frees the rows and, for a cleaner that is kept (open or not), the
    /// reducer asks for a new scan.
    fn on_clean_event(&mut self, event: CleanEvent, now: Instant) {
        match event {
            CleanEvent::Scanned(scan) => {
                self.clean.scanning = false;
                self.clean.scan = Some(scan);
            }
            CleanEvent::Done(report) => {
                // The old sizes are no longer true.
                self.clean.scan = None;
                let notice = {
                    let r = &self.renderer;
                    let kind = NoticeKind::CleanDone;
                    let fits = |s: &str| r.notice_text_fits(&kind, false, s);
                    clean::done_notice(i18n::lang(), &report, &fits)
                };
                // The notice first: a result the notice covers is not yet
                // seen. The reducer then asks for the new scan itself
                // (`Effect::OpenCleaner`), with the panel open or shut.
                self.apply(Event::Notice(notice), now);
                self.apply(Event::CleanFinished, now);
            }
        }
    }

    /// The Mole thread, started on first use; `None` when it cannot run.
    fn mole_worker(&mut self) -> Option<&MoleWorker> {
        if self.mole.worker.is_none() {
            self.mole.worker = MoleWorker::spawn(self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        }
        self.mole.worker.as_ref()
    }

    /// Looks for Mole on the worker (PATH may name slow folders).
    fn mole_detect(&mut self) -> Option<Event> {
        if self.mole_worker().is_some_and(MoleWorker::detect) {
            return None;
        }
        self.mole.worker = None;
        Some(Event::MoleFound(false))
    }

    /// A new dry-run; one running stops and its output no longer counts.
    /// The default protections go into Mole's whitelist first, once.
    fn mole_preview(&mut self, opts: MoleOpts) -> Option<Event> {
        self.mole_stop_preview();
        self.mole.log.clear();
        self.mole.report = None;
        let failed = Some(Event::MolePreviewed { opts, ok: false });
        let Some(install) = self.mole.install.clone() else {
            return failed;
        };
        let protect = self.config.mole_protect_defaults && !self.config.mole_defaults_added;
        let run = self.mole.run;
        if self
            .mole_worker()
            .is_some_and(|w| w.preview(run, install, opts, protect))
        {
            return None;
        }
        self.mole.worker = None;
        failed
    }

    /// The cleaner closed or left Mole mode: a dry-run stops, the preview goes.
    fn mole_stop_preview(&mut self) {
        if let Some(worker) = &self.mole.worker {
            worker.stop_preview();
        }
        self.mole.preview = None;
        self.mole.run += 1;
    }

    /// The confirmed clean (the reducer's second click), with the finished
    /// preview for these options; without one nothing runs.
    fn mole_clean(&mut self, opts: MoleOpts) -> Option<Event> {
        let preview = self.mole.preview.take().filter(|p| p.opts() == opts);
        let (Some(install), Some(preview)) = (self.mole.install.clone(), preview) else {
            return Some(Event::MoleFinished { ok: false });
        };
        self.mole.run += 1;
        self.mole.log.clear();
        self.mole.report = None;
        let run = self.mole.run;
        let sent = self
            .mole_worker()
            .is_some_and(|w| w.clean(run, install, preview, opts, Confirmed::second_click()));
        if !sent {
            self.mole.worker = None;
            return Some(Event::MoleFinished { ok: false });
        }
        self.bata.brain.mole_running = true;
        None
    }

    /// What the Mole thread found, printed or measured.
    fn on_mole_event(&mut self, event: MoleEvent, now: Instant) {
        match event {
            MoleEvent::Detected(found) => {
                let ok = found.is_some();
                self.mole.install = found;
                self.apply(Event::MoleFound(ok), now);
            }
            MoleEvent::DefaultsAdded => {
                self.config.mole_defaults_added = true;
                self.save_config();
            }
            MoleEvent::Lines { run, lines } => {
                if run == self.mole.run {
                    let added = lines.len();
                    self.mole.log.extend(lines);
                    while self.mole.log.len() > mole::LOG_CAP {
                        self.mole.log.pop_front();
                    }
                    // A log scrolled up stays on what it shows, panel open
                    // or shut (the offset is kept for the return); at the
                    // newest (the usual case) nothing is applied. The drain
                    // refreshes after any Mole event either way.
                    if self.state.mole_log_scroll() > 0 {
                        self.apply(Event::MoleLogLines(added), now);
                    }
                }
            }
            MoleEvent::Previewed { run, opts, result } => {
                if run != self.mole.run {
                    return;
                }
                let ok = result.is_ok();
                self.mole.preview = result.ok();
                self.apply(Event::MolePreviewed { opts, ok }, now);
            }
            MoleEvent::Cleaned { run, report } => {
                self.bata.brain.mole_running = false;
                let ok = report.run.ok();
                let notice = {
                    let r = &self.renderer;
                    let kind = if ok {
                        NoticeKind::MoleDone
                    } else {
                        NoticeKind::MoleFailed
                    };
                    let fits = |title: bool, s: &str| r.notice_text_fits(&kind, title, s);
                    mole::done_notice(i18n::lang(), &report, &fits)
                };
                if run == self.mole.run {
                    self.mole.report = Some(report);
                }
                // The notice first: a result the notice covers is not yet
                // seen (the cleaner ends only after it was).
                self.apply(Event::Notice(notice), now);
                self.apply(Event::MoleFinished { ok }, now);
            }
        }
    }

    /// The CLI page's input has the keyboard for real: the island is the
    /// foreground window and the hidden EDIT its focus.
    fn cli_has_keyboard(&self) -> bool {
        self.cli_input.as_ref().is_some_and(CliInput::has_focus)
            && unsafe { GetForegroundWindow() } == self.hwnd
    }

    /// A click on the CLI page's terminal: the island activates (allowed,
    /// since this process got the click) and the hidden EDIT takes the
    /// keyboard. The window that had it is remembered for later. An open
    /// editor comes forward instead: it may hold unsaved text.
    fn focus_cli_input(&mut self) -> Option<Event> {
        if editor::focus_open_editor() {
            return None;
        }
        let input = self.cli_input.as_ref()?;
        unsafe {
            let fg = GetForegroundWindow();
            if fg != self.hwnd {
                self.cli_prev = (!fg.is_invalid()).then_some(fg);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
        let ok = input.focus() && unsafe { GetForegroundWindow() } == self.hwnd;
        if !ok {
            self.return_keyboard();
            return None;
        }
        (!self.state.cli_focused()).then_some(Event::CliFocus(true))
    }

    /// Gives the keyboard back to the window that had it before the input
    /// took it, if the island still has it (a click elsewhere already chose
    /// a new one). With nowhere to go back to, keys at least stop reaching
    /// the hidden input.
    fn return_keyboard(&mut self) {
        let prev = self.cli_prev.take();
        unsafe {
            if GetForegroundWindow() != self.hwnd {
                return;
            }
            if let Some(prev) = prev
                && IsWindow(Some(prev)).as_bool()
                && IsWindowVisible(prev).as_bool()
                && SetForegroundWindow(prev).as_bool()
            {
                return;
            }
            let input_has = |i: &Option<CliInput>| i.as_ref().is_some_and(CliInput::has_focus);
            if input_has(&self.cli_input)
                || input_has(&self.clip_input)
                || input_has(&self.notif_input)
                || input_has(&self.search_input)
            {
                let _ = SetFocus(None);
            }
        }
    }

    /// Esc, or the reducer letting go: the panel unlocks and the keyboard
    /// goes back.
    fn release_cli(&mut self) {
        self.return_keyboard();
        if self.state.cli_focused() {
            self.apply_now(Event::CliFocus(false));
        }
    }

    fn stop_cli(&self) {
        if let Some(run) = &self.cli_run {
            run.kill();
        }
    }

    /// Output lines the CLI page shows at once, for PageUp / PageDown and
    /// how far the output can scroll.
    fn cli_rows(&self) -> usize {
        let s = shape_for(self.config.style, self.panel_kind());
        let strip = self.state.waiting_strip().is_some();
        cli_layout(s, strip, &CliHead::default()).rows.max(1)
    }

    fn cli_scroll(&mut self, lines: i32) {
        let max = cli_panel::max_scroll(self.cli.output.lines().len(), self.cli_rows());
        self.apply_now(Event::CliScroll { lines, max });
    }

    /// Earlier commands starting with the input, while it is being typed
    /// (not while Up / Down shows a history entry).
    fn cli_suggestions(&self, typed: &str) -> Vec<&str> {
        if self.cli_browsing || !self.state.cli_focused() {
            return Vec::new();
        }
        cli_panel::suggestions(self.cli_history.items(), typed, MAX_SUGGESTIONS)
    }

    /// Puts `text` in the input as the island's own change (not an edit
    /// that would end history browsing).
    fn set_cli_text(&mut self, text: &str) {
        self.cli_text = text.to_owned();
        if let Some(input) = &self.cli_input {
            input.set_text(text);
        }
    }

    /// A key the CLI page handles itself, from the hidden EDIT.
    fn on_cli_key(&mut self, key: InputKey) {
        let typed = self
            .cli_input
            .as_ref()
            .map(CliInput::text)
            .unwrap_or_default();
        match key {
            InputKey::Enter => {
                let cmd = typed.trim().to_owned();
                self.cli_cursor.reset();
                self.cli_browsing = false;
                self.cli_pick = 0;
                self.set_cli_text("");
                if !cmd.is_empty() {
                    self.run_cli(&cmd);
                    // A new command shows from the bottom.
                    self.cli_scroll(-i32::MAX);
                }
            }
            InputKey::Up | InputKey::Down => {
                let step = if key == InputKey::Up { -1 } else { 1 };
                let shown = self.cli_suggestions(&typed).len();
                if shown > 0 {
                    self.cli_pick = cli_panel::step_selection(self.cli_pick, shown, step);
                    self.redraw_cli();
                    return;
                }
                let items = self.cli_history.items();
                let next = if key == InputKey::Up {
                    self.cli_cursor.up(items, &typed)
                } else {
                    self.cli_cursor.down(items)
                };
                if let Some(text) = next {
                    // Suggestions stay away until the entry is edited.
                    self.cli_browsing = true;
                    self.set_cli_text(&text);
                }
            }
            InputKey::Tab => {
                let taken = cli_panel::accept(&self.cli_suggestions(&typed), self.cli_pick);
                if let Some(text) = taken {
                    self.cli_pick = 0;
                    self.set_cli_text(&text);
                }
            }
            InputKey::Stop => self.stop_cli(),
            // Only the Notifikasi page's input sends Delete.
            InputKey::Delete => {}
            // The folder menu closes first; the next Esc lets go.
            InputKey::Escape if self.state.cli_menu() => {
                self.apply_now(Event::Click(Hit::CliMenuDismiss))
            }
            InputKey::Escape => self.release_cli(),
            InputKey::PageUp => self.cli_scroll(self.cli_rows() as i32),
            InputKey::PageDown => self.cli_scroll(-(self.cli_rows() as i32)),
            InputKey::Blur => {
                // Another window took the keyboard (a click outside the panel,
                // Alt+Tab): no foreground to hand back.
                if self.state.cli_focused() && !self.cli_has_keyboard() {
                    self.cli_prev = None;
                    self.apply_now(Event::CliFocus(false));
                }
            }
        }
    }

    /// The input's text or selection changed: the page redraws, and an edit
    /// by the user ends history browsing and restarts the suggestions.
    fn on_cli_changed(&mut self) {
        cli_input::redraw_done();
        let typed = self
            .cli_input
            .as_ref()
            .map(CliInput::text)
            .unwrap_or_default();
        if typed != self.cli_text {
            self.cli_text = typed;
            self.cli_browsing = false;
            self.cli_cursor.reset();
            self.cli_pick = 0;
        }
        self.redraw_cli();
    }

    /// A hidden EDIT's text or selection changed: the CLI page and the
    /// Clipboard search each follow their own (and redraw only when shown).
    fn on_input_changed(&mut self) {
        self.on_cli_changed();
        self.on_clip_changed();
        self.on_notif_changed();
        // The search's field is also Susun's name field, which is up with
        // the search closed.
        if self.state.search_open() || self.arrange_naming() {
            self.on_search_changed();
        }
    }

    fn redraw_cli(&mut self) {
        if self.state.view() == View::Panel(Page::Cli) && !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// A command from the CLI page: a builtin, or a run in the shell.
    fn run_cli(&mut self, cmd: &str) {
        self.cli_history.push(cmd);
        // A history that cannot be written only loses the history.
        let _ = self.cli_history.save(&cli::history_path());
        let now = Instant::now();
        // While a command runs, `Session` keeps all of this off the card.
        match cli::builtin(cmd) {
            Some(Builtin::Clear) => {
                self.cli.clear();
                self.cli_clock.clear();
            }
            Some(Builtin::Cd(arg)) => {
                self.begin_cli(cmd);
                match cli::resolve_cd(&self.cli_cwd, arg.as_deref(), &cli::home()) {
                    // A manual cd wins over Explorer until the panel closes.
                    Ok(dir) => self.set_cli_folder(dir),
                    Err(arg) => self.cli.note(cli::cd_error(&arg)),
                }
            }
            None if self.cli.running().is_some() => {
                self.begin_cli(cmd);
                self.cli.note(tr(Key::CliBusy).into());
            }
            None => {
                self.begin_cli(cmd);
                self.cli_runs += 1;
                let id = self.cli_runs;
                let shell = self.config.cli_shell;
                match cli::spawn(id, cmd, &self.cli_cwd, shell, self.cli_sink()) {
                    Ok(run) => {
                        self.cli.started(id);
                        self.cli_run = Some(run);
                        self.cli_clock.start(now);
                        self.bata.brain.cli_started();
                        // The folder a command ran in is the user's last one.
                        let _ = cli::save_cwd(&cli::cwd_path(), &self.cli_cwd);
                        let dir = self.cli_cwd.clone();
                        self.remember_recent(&dir);
                        // A command ran here: this visit stays here (a slow
                        // Explorer read must not move it afterwards), and
                        // the folder is the user's own now (cli-cwd.txt).
                        self.cli_pinned = true;
                        self.cli_from_explorer = false;
                        // The folder cannot change mid-command.
                        if self.state.cli_menu() {
                            self.apply(Event::Click(Hit::CliMenuDismiss), now);
                        }
                    }
                    Err(error) => {
                        let line = i18n::cli_start_failed(i18n::lang(), &error.to_string());
                        self.cli.note(line);
                    }
                }
            }
        }
        self.note_cli_output(now);
        self.refresh(now);
    }

    /// Echoes `cmd`, remembering that line as a command.
    fn begin_cli(&mut self, cmd: &str) {
        self.cli_echoes.mark(&self.cli.output);
        self.cli.begin(cmd);
    }

    /// Where a run's threads send their output: the island's queue, waking
    /// it once per burst.
    fn cli_sink(&self) -> cli::Sink {
        let tx = self.tx.clone();
        let wake = poster(self.hwnd, WM_APP_EVENT);
        let pending = self.cli_wake.clone();
        Arc::new(move |event| {
            if tx.send(Event::Cli(event)).is_ok() && !pending.swap(true, Ordering::SeqCst) {
                wake();
            }
        })
    }

    fn on_cli_event(&mut self, event: CliEvent) {
        match event {
            CliEvent::Output { run, lines } => {
                self.cli.lines(run, lines);
            }
            CliEvent::Exit { run, code, killed } => {
                if self.cli.exit(run, code, killed) {
                    // The command is over; what it started may go on.
                    let now = Instant::now();
                    self.cli_run = None;
                    self.cli_clock.end(code, killed, now);
                    self.bata
                        .brain
                        .cli_finished(code == Some(0) && !killed, now);
                }
            }
        }
    }

    /// Runs `action` on `TIMER_SHORTCUT`, once the panel has collapsed;
    /// replaces a pending one, so only the last click runs.
    fn queue_shortcut(&mut self, action: shortcuts::Action) {
        // A screen picker still waiting for the timer is replaced and will
        // never answer: the screen recording goes back to idle.
        if matches!(self.shortcut, Some(shortcuts::Action::ScreenPick { .. })) {
            self.screen_pick_done(None);
        }
        // The Screenshot tile: Vale's own capture unless Settings chose
        // Snipping Tool ("Alat screenshot").
        let action = match action {
            shortcuts::Action::Open("ms-screenclip:")
                if self.config.screenshot.tool == config::ShotTool::Vale =>
            {
                shortcuts::Action::Capture
            }
            other => other,
        };
        let delay = shortcuts::delay_ms(&action);
        self.shortcut = Some(action);
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_SHORTCUT, delay, None);
        }
    }

    /// Asks the sampler thread (started on the first request) for a
    /// reading. False when it cannot run; the next request starts it again.
    fn request_system(&mut self, kind: SampleKind) -> bool {
        if self.system.sampler.is_none() {
            self.system.sampler = Sampler::spawn(self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        }
        let sent = self
            .system
            .sampler
            .as_ref()
            .is_some_and(|s| s.request(kind));
        if !sent {
            self.system.sampler = None;
        }
        sent
    }

    /// The System page just showed: a sample at once (a stale reading goes
    /// first); it left: the `LIVE_MS` one-shot goes (a sample still on its
    /// way is kept but re-arms nothing).
    fn sync_system_page(&mut self) {
        let wanted = system_sample_wanted(&self.state.view(), self.state.cleaner().is_some());
        self.system.requests = requests_while(wanted, self.system.requests);
        match system_page_plan(wanted, self.system.page_timer, self.system.page_pending) {
            PagePlan::Request => {
                // Opened again after a while: dashes, not the last figures.
                if self.system.requests == 0 && self.system.forget_stale(Instant::now()) {
                    self.redraw_system_page();
                }
                let kind = crate::system::kind_for_request(self.system.requests);
                self.system.requests = self.system.requests.wrapping_add(1);
                self.system.page_pending = self.request_system(kind);
            }
            PagePlan::Stop => {
                self.system.page_timer = false;
                // The next time the page shows starts with a full reading.
                self.system.requests = 0;
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_SYSTEM_PAGE);
                }
            }
            PagePlan::Keep => {}
        }
    }

    /// Sends `req` to the audio thread, starting it on the first one.
    fn audio_request(&mut self, req: audio::Req) -> bool {
        if self.audio.is_none() {
            self.audio = audio::Audio::spawn(self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        }
        let sent = self.audio.as_ref().is_some_and(|a| a.request(req));
        if !sent {
            // A thread that is gone answers nothing: no reading is coming.
            self.audio = None;
            self.mixer_pending = false;
            self.volume_pending = 0;
        }
        sent
    }

    /// Asks for the volumes unless a reading is already on its way.
    fn mixer_scan(&mut self) {
        if !self.mixer_pending {
            self.mixer_seq += 1;
            self.mixer_pending = self.audio_request(audio::Req::Scan(self.mixer_seq));
        }
    }

    /// Sends a level or a mute; readings asked before it are stale.
    fn audio_set(&mut self, req: audio::Req) {
        self.mixer_seq += 1;
        self.mixer_set_seq = self.mixer_seq;
        self.audio_request(req);
    }

    /// The Mixer page showed: read at once and every `MIXER_MS`; it left: the
    /// timer goes, and so does a drag (the button may come up elsewhere).
    fn sync_mixer(&mut self) {
        let wanted = self.state.view() == View::Panel(Page::Mixer);
        if wanted && !self.mixer_timer {
            self.mixer_timer = true;
            self.mixer_resort = true;
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_MIXER, MIXER_MS, None);
            }
            self.mixer_scan();
        } else if !wanted && self.mixer_timer {
            self.mixer_timer = false;
            self.mixer_wheel_sum = None;
            self.end_mixer_drag();
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_MIXER);
            }
        }
    }

    /// The audio thread's reading. A drag keeps what the pointer set until
    /// it lets go, and a reading asked before the last level or mute may
    /// still hold the old value, so neither replaces the rows; otherwise the
    /// rows are the reading, in the order the open page already shows.
    fn on_audio(&mut self, asked: u64, rows: Option<Vec<audio::MixerRow>>) {
        self.mixer_pending = false;
        let master = rows
            .as_ref()
            .and_then(|r| r.iter().find(|row| row.target == audio::Target::Master))
            .map(|row| row.level);
        let mut changed = false;
        if self.mixer_drag.is_none() && !audio::reading_is_stale(asked, self.mixer_set_seq) {
            let next = match (rows, &self.mixer) {
                (Some(fresh), Some(shown)) if !self.mixer_resort => {
                    Some(audio::keep_order(shown, fresh))
                }
                (rows, _) => rows,
            };
            changed = next != self.mixer;
            self.mixer = next;
            self.mixer_resort = false;
        }
        // A wheel turn waited for this reading.
        let waiting = std::mem::take(&mut self.volume_pending);
        if let (Some(level), true) = (master, waiting != 0) {
            self.volume_cache = Some(level);
            self.volume_step(waiting);
        }
        // The page reads every second; only a reading that differs draws.
        if changed && self.state.view() == View::Panel(Page::Mixer) && !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// The wheel over the music (the compact notch, or the Music page's
    /// cover) turns the master volume, 2 % a notch. The level is read once
    /// and kept while the plate shows; the first turn waits for the reading.
    fn volume_wheel(&mut self, delta: i32) {
        let points = audio::wheel_points(&mut self.volume_sum, delta);
        if points == 0 {
            return;
        }
        if self.volume_cache.is_some() {
            self.volume_step(points);
        } else {
            self.volume_pending = self.volume_pending.saturating_add(points);
            self.mixer_scan();
            // No reading is on its way (the audio thread is gone): the turn
            // is dropped, not kept for whatever reading comes minutes later.
            if !self.mixer_pending {
                self.volume_pending = 0;
            }
        }
    }

    fn volume_step(&mut self, points: i32) {
        let Some(level) = self.volume_cache else {
            return;
        };
        let next = audio::move_points(level, points);
        self.volume_cache = Some(next);
        self.audio_set(audio::Req::Level(audio::Target::Master, next));
        self.volume_hud = Some(next);
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_VOLUME, VOLUME_HUD_MS, None);
        }
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// `TIMER_VOLUME`: the plate goes.
    fn on_volume_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_VOLUME);
        }
        self.volume_hud = None;
        self.volume_cache = None;
        self.volume_pending = 0;
        self.volume_sum = 0;
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// Row `i` of the Mixer takes `level`: shown at once, set by the audio
    /// thread.
    fn mixer_set_level(&mut self, i: usize, level: f32) {
        let level = audio::clamp_level(level);
        let Some(row) = self.mixer.as_mut().and_then(|rows| rows.get_mut(i)) else {
            return;
        };
        if row.level == level {
            return;
        }
        row.level = level;
        let target = row.target.clone();
        // No draw here: the effect comes out of `apply`, and the `refresh`
        // after it draws once.
        self.audio_set(audio::Req::Level(target, level));
    }

    /// Like `mixer_set_level`, drawn by the `refresh` after the click.
    fn mixer_toggle_mute(&mut self, i: usize) {
        let Some(row) = self.mixer.as_mut().and_then(|rows| rows.get_mut(i)) else {
            return;
        };
        row.muted = !row.muted;
        let (target, muted) = (row.target.clone(), row.muted);
        self.audio_set(audio::Req::Mute(target, muted));
    }

    /// Row `i`'s level: `None` when there is no such row.
    fn mixer_level(&self, i: usize) -> Option<f32> {
        self.mixer.as_ref()?.get(i).map(|row| row.level)
    }

    /// A press on a Mixer slider sets the level under the pointer and starts
    /// a drag of that row's target; moves with the button down follow it
    /// (`on_mouse_move`), outside the island too: the window takes the
    /// capture until the release.
    fn mixer_press(&mut self, hit: Option<&Hit>, lparam: LPARAM) {
        self.end_mixer_drag();
        let Some(Hit::MixerSlider(i)) = hit else {
            return;
        };
        let Some(row) = self.mixer.as_ref().and_then(|rows| rows.get(*i)) else {
            return;
        };
        self.mixer_drag = Some(row.target.clone());
        unsafe {
            SetCapture(self.hwnd);
        }
        self.mixer_follow(lparam);
    }

    /// Ends a slider drag and lets the capture go. `ReleaseCapture` sends
    /// `WM_CAPTURECHANGED` back while `App` is borrowed; that nested call
    /// finds it busy and returns, and the drag is already over.
    fn end_mixer_drag(&mut self) {
        if self.mixer_drag.take().is_some() {
            unsafe {
                let _ = ReleaseCapture();
            }
        }
    }

    /// `WM_CAPTURECHANGED`: another window took the capture (or it was let
    /// go): the drag is over.
    fn on_capture_lost(&mut self) {
        self.mixer_drag = None;
    }

    /// The dragged slider's level at the pointer; the drag ends when the
    /// left button is no longer down or its row has gone.
    fn mixer_follow(&mut self, lparam: LPARAM) {
        let Some(target) = self.mixer_drag.as_ref() else {
            return;
        };
        let row = self
            .mixer
            .as_ref()
            .and_then(|rows| rows.iter().position(|r| r.target == *target));
        let Some(i) = row.filter(|_| left_button_down()) else {
            self.end_mixer_drag();
            return;
        };
        let (x, _) = client_xy(lparam);
        // A move that leaves the level where it is (sliding along one
        // percent, or past an end) neither sets nor draws anything.
        if let Some(level) = self.renderer.mixer_level_at(i, x as f32)
            && self.mixer_level(i) != Some(level)
        {
            self.apply_now(Event::MixerLevel(i, level));
        }
    }

    /// The wheel over a Mixer row steps its level 5 % a notch (up is
    /// louder). A touchpad's small deltas add up per row until they make a
    /// notch.
    fn mixer_wheel(&mut self, delta: i32, row: usize) {
        let Some(current) = self.mixer.as_ref().and_then(|rows| rows.get(row)) else {
            return;
        };
        let (target, level) = (current.target.clone(), current.level);
        let mut sum = match self.mixer_wheel_sum.take() {
            Some((on, sum)) if on == target => sum,
            _ => 0,
        };
        let notches = audio::wheel_notches(&mut sum, delta);
        self.mixer_wheel_sum = Some((target, sum));
        if notches == 0 {
            return;
        }
        // Nothing to do at an end.
        let next = audio::step_level(level, notches);
        if next != level {
            self.apply_now(Event::MixerLevel(row, next));
        }
    }

    /// `TIMER_SYSTEM_WARN`: the cheap disk and RAM read for the warnings.
    fn on_system_warn_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SYSTEM_WARN);
        }
        // One already on its way re-arms the timer when it arrives.
        if self.system.warn_pending {
            return;
        }
        self.system.warn_pending = self.request_system(SampleKind::Warn);
        if !self.system.warn_pending {
            self.arm_system_warn(SYSTEM_WARN_MS);
        }
    }

    fn arm_system_warn(&self, ms: u32) {
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_SYSTEM_WARN, ms, None);
        }
    }

    /// A reading came back. Every reading feeds the warnings (a notice for
    /// each new crossing). A page sample is kept for the page, which redraws
    /// and asks again in `SYSTEM_PAGE_MS` while it shows; a warning check
    /// re-arms its timer, sooner while RAM's minute is being confirmed.
    fn on_system_sample(&mut self, sample: SystemSample, now: Instant) {
        // A cheap reading has no drives: the warnings wait for a full one.
        let check = if sample.kind == SampleKind::Live {
            Default::default()
        } else {
            self.system
                .monitor
                .update(&sample.drives, sample.ram_percent(), now)
        };
        match sample.kind {
            SampleKind::Page | SampleKind::Live => {
                self.system.page_pending = false;
                self.system.history.push(sample.cpu, now);
                self.system.ease.update(&sample, now);
                match (&mut self.system.sample, sample.kind) {
                    // A cheap reading refreshes the figures it measured and
                    // keeps the drives and processes of the full one.
                    (Some(kept), SampleKind::Live) => {
                        kept.cpu = sample.cpu;
                        kept.ram_used = sample.ram_used;
                        kept.ram_total = sample.ram_total;
                        kept.uptime = sample.uptime;
                    }
                    (slot, _) => *slot = Some(sample),
                }
                if system_sample_wanted(&self.state.view(), self.state.cleaner().is_some()) {
                    // The figures glide: the frame clock draws them; with
                    // nothing moving it is one plain draw.
                    if self.system.ease.running(now) {
                        self.ensure_clock();
                    } else if !self.animating() {
                        self.draw(now);
                    }
                    self.system.page_timer = true;
                    unsafe {
                        SetTimer(Some(self.hwnd), TIMER_SYSTEM_PAGE, SYSTEM_PAGE_MS, None);
                    }
                }
                // The page may close before the minute is up: the warning
                // timer confirms it then.
                if check.recheck.is_some() && !self.system.warn_pending {
                    self.arm_system_warn(warn_wait_ms(check.recheck));
                }
            }
            SampleKind::Warn => {
                self.system.warn_pending = false;
                self.arm_system_warn(warn_wait_ms(check.recheck));
            }
        }
        for alert in &check.alerts {
            self.system.raised = true;
            self.apply(Event::Notice(alert_notice(i18n::lang(), alert)), now);
        }
    }

    /// The shortcut `TIMER_SHORTCUT` was waiting for, taken once.
    fn take_shortcut(&mut self) -> Option<shortcuts::Action> {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SHORTCUT);
        }
        self.shortcut.take()
    }

    fn on_editor_result(&mut self, result: EditorResult) {
        let now = Instant::now();
        let closes = result.closes_editor();
        match result {
            EditorResult::NoteSaved { id, text } => self.save_note(id.as_deref(), &text),
            EditorResult::NoteDeleted { id } => {
                if self.notes.delete(&id) {
                    self.persist_notes(now);
                    // A note on the desktop goes with it.
                    self.widgets_sync();
                }
            }
            EditorResult::EventSave {
                event_id,
                calendar_id,
                draft,
            } => {
                let reply = self.editor.map_or(0, |hwnd| hwnd.0 as isize);
                let request = SyncRequest::Save {
                    event_id,
                    calendar_id,
                    draft,
                    reply,
                };
                if !self.sync.as_ref().is_some_and(|sync| sync.send(request)) {
                    // No sync thread to answer: the editor must not wait forever.
                    let failed = Err(i18n::save_failed_because(
                        i18n::lang(),
                        tr(Key::ReasonSyncStopped),
                    ));
                    if let Some(message) = post_status(reply, failed) {
                        self.apply(Event::Notice(save_failed_notice(message)), now);
                    }
                }
            }
            EditorResult::ReminderSaved {
                id,
                text,
                date,
                time,
                repeat,
                days,
            } => {
                let unix = Local::now().timestamp();
                // Keeps the time as set, even on a day a DST change skips it.
                if self
                    .reminders
                    .upsert_local(id.as_deref(), &text, date, time, repeat, &days, unix)
                    .is_some()
                {
                    self.persist_reminders(now);
                    self.settle_reminders(now);
                }
            }
            EditorResult::ReminderDeleted { id } => {
                if self.reminders.delete(&id) {
                    self.persist_reminders(now);
                    self.settle_reminders(now);
                }
            }
            EditorResult::Closed => {}
        }
        if closes {
            self.editor = None;
            self.apply(Event::EditorOpen(false), now);
        }
        self.refresh(now);
    }

    /// An untouched note is not saved: `upsert` would still move it to the top.
    fn save_note(&mut self, id: Option<&str>, text: &str) {
        if !note_changed(&self.notes, id, text) {
            return;
        }
        // The note's id from here on is the one `upsert` returns (a new note, or
        // one deleted meanwhile, gets a fresh one; `None` = emptied and deleted).
        // The editor is gone, so nothing else holds an id to update.
        let _id = self.notes.upsert(id, text, Local::now().timestamp());
        self.persist_notes(Instant::now());
        // The same note on the desktop shows the new text (or goes, emptied).
        self.widgets_sync();
    }

    fn persist_notes(&mut self, now: Instant) {
        // Whatever the desktop notes had waiting goes out with this write.
        self.widgets_saved();
        match self.notes.save() {
            // What the file no longer refers to goes from the images folder.
            Ok(()) => self.widgets_images_release(),
            Err(error) => {
                let notice = island_notice("notes", tr(Key::NotesSaveFailed), &error.to_string());
                self.apply(Event::Notice(notice), now);
            }
        }
    }

    /// The editor died without its last result (never expected): the panel
    /// must not stay pinned open.
    fn check_editor(&mut self) {
        if let Some(hwnd) = self.editor
            && !unsafe { IsWindow(Some(hwnd)) }.as_bool()
        {
            self.editor = None;
            self.apply_now(Event::EditorOpen(false));
        }
    }

    fn check_date(&mut self) {
        let today = Local::now().date_naive();
        if today != self.state.today() {
            self.apply_now(Event::Today(today));
            // A new day may follow a clock or time-zone change.
            self.fire_reminders(false);
        }
    }

    fn stop_sync(&mut self) {
        if let Some(sync) = self.sync.take() {
            sync.send(SyncRequest::Shutdown);
        }
    }

    fn apply_now(&mut self, event: Event) {
        let now = Instant::now();
        self.apply(event, now);
        self.refresh(now);
    }

    /// Retargets the animation at the state's view and re-arms the notice deadline.
    fn refresh(&mut self, now: Instant) {
        self.rec_follow_page(self.state.panel_page_under());
        self.screen_follow_page(self.state.panel_page_under());
        self.draw_gen = self.draw_gen.wrapping_add(1);
        // The sleep clock follows mouse and keyboard input anywhere on the PC.
        self.fold_pc_input(now);
        self.sync_lyrics(now);
        // A busy toast's logo beside the lyric line: the width, the box and
        // the scroll plan are worked out with it. After `sync_lyrics`, whose
        // `LyricsReady` may change the view the slot is read from; before
        // `sync_lyric_line`, which fits the line and re-plans the scroll
        // (`TIMER_LYRIC_SCROLL`, the frame clock) with the flag as it is now.
        let lyric_slot =
            self.state.toast_slot().is_some() && self.state.view() == View::MediaLyric;
        self.lyric_fit.set_slot(lyric_slot);
        // The reducer is the authority on where the pointer is for the toast
        // (it lets go when a toast ends or the pointer leaves); the next
        // mouse move compares against that.
        self.toast_over = self.state.toast_pointer();
        self.rec_hot_over = self.state.rec_mute_hot();
        if let Some(n) = self.state.toast_slot() {
            self.slot_keep = Some(n.clone());
        }
        // The notch peek only lives on a quiet compact view; anything else
        // (a notice, the panel, fullscreen, stay-behind) ends it at once.
        self.peek_guard();
        let was_eyes = std::mem::replace(&mut self.eyes, self.peek.is_shrunk());
        let eyes_swap = was_eyes != self.eyes;
        // After the eyes are settled: the shrunk notch draws no lyric, so its
        // crossfade and scroll need no frames; back at full size they go on
        // from where the line is now.
        self.sync_lyric_line(now);
        let view = self.state.view();
        // The cat's look goes back to the centre when it cannot follow.
        self.sync_look(now);
        // The eyes start centred at every shrink and every restore, and look
        // only while the shrunk notch may follow the pointer.
        if eyes_swap {
            self.eyes_look.snap_to_centre();
            self.eyes_look_at = None;
        }
        self.sync_eyes_look();
        // Bata: an alert that just appeared bounces; a lift or bounce needs frames.
        let face = self.robot_frame(&view, now).map(|r| r.expr);
        let moves = self.motion().moves();
        let token = match &view {
            View::Notice(n) => {
                if self.notice_shown.as_ref() != Some(n) {
                    self.notice_serial += 1;
                    self.notice_shown = Some(n.clone());
                }
                Some(self.notice_serial)
            }
            _ => {
                self.notice_shown = None;
                None
            }
        };
        self.bata.brain.observe(face, token, now, moves);
        if self.robot_moving(now) {
            self.ensure_clock();
        }
        // The notice's ✕ comes with the pointer on the island and goes with
        // it; its fade needs frames only while it runs.
        self.sync_close_fade(now);
        let notch = self.notch_size();
        if view == View::MediaLyric {
            let max_w = self.lyric_max_w();
            self.lyric_fit.set_min(notch.w);
            self.lyric_fit.refit(self.config.robot, max_w);
        }
        let kind = match &view {
            View::Hidden => None,
            View::Idle => Some(ShapeKind::Idle),
            View::MediaCompact => Some(ShapeKind::MediaCompact),
            View::MediaLyric => Some(ShapeKind::MediaLyric),
            View::PomodoroCompact => Some(ShapeKind::PomodoroCompact),
            // The countdown takes the REC view's shape: the recording it
            // leads to shows in the same place.
            View::RecordCompact | View::ScreenCountdown(_) => Some(ShapeKind::RecordCompact),
            // The caption line is the REC view's shape, widened below.
            View::CaptionCompact => Some(ShapeKind::RecordCompact),
            View::Panel(_) => Some(self.panel_kind()),
            View::Notice(n) if matches!(n.kind, NoticeKind::Screenshot { .. }) => {
                Some(ShapeKind::NoticeShot)
            }
            View::Notice(_) => Some(ShapeKind::Notice),
        };
        // Compact shapes widen to hold the robot at their left; the lyric
        // notch is as wide as its line (robot included).
        let (style, robot) = (self.config.style, self.config.robot);
        let caption_wide = match &view {
            View::CaptionCompact => true,
            View::RecordCompact => self.state.rec_caption().is_some(),
            _ => false,
        };
        let target = kind.map(|kind| match kind {
            // Shrunk to the eyes (only ever on a compact view).
            _ if self.eyes => shape_sized(style, notch, ShapeKind::Eyes),
            ShapeKind::MediaLyric => lyric_shape_sized(style, notch, self.lyric_fit.width()),
            // The idle island grows to the left for the typing cat's laptop.
            ShapeKind::Idle => self.idle_shape(now),
            // The compact views widen to hold the busy toast's logo.
            ShapeKind::MediaCompact | ShapeKind::PomodoroCompact | ShapeKind::RecordCompact
                if self.state.toast_slot().is_some() =>
            {
                let shape = shape_with_robot_sized(style, notch, kind, robot);
                with_toast_slot(if caption_wide {
                    with_rec_caption(shape)
                } else {
                    shape
                })
            }
            // The REC view with a caption line, and the caption view: room
            // for the line.
            ShapeKind::RecordCompact if caption_wide => {
                with_rec_caption(shape_with_robot_sized(style, notch, kind, robot))
            }
            _ => shape_with_robot_sized(style, notch, kind, robot),
        });
        // The logo's room in the target: a lyric line at its cap keeps the
        // notch's width and gives the room up from its box, so the slot
        // toggling there is a morph of its own.
        let slot_to = if target.is_some() && !self.eyes && self.state.toast_slot().is_some() {
            TOAST_SLOT_EXTRA
        } else {
            0.0
        };
        let retarget = match target {
            Some(shape) => {
                self.anim.to != shape
                    || self.anim.to_opacity != 1.0
                    || self.slot_share.to != slot_to
            }
            None => self.anim.to_opacity != 0.0,
        };
        // Hidden, something else on screen, or the notch swapped with the
        // eyes: a press made before must not pair with a release on what
        // shows now.
        if press_ends(&view, &self.drawn, eyes_swap) && !progress_in_place(&view, &self.drawn) {
            self.click.clear();
        }
        // Only the width changes while the lyric view stays: a quicker morph.
        let resize = view == View::MediaLyric
            && self.drawn == View::MediaLyric
            && self.anim.to_opacity == 1.0
            && !eyes_swap;
        let view_same = view == self.drawn || progress_in_place(&view, &self.drawn);
        // A compact view or notice replacing another fades in with its
        // morph, and a panel closing into one fades out as the shape shrinks.
        let was_panel = match &self.drawn {
            View::Panel(page) => Some(*page),
            _ => None,
        };
        let is_panel = matches!(view, View::Panel(_));
        // Shrinking to the eyes or coming back swaps the content like a view
        // change does: what shows fades in with the morph.
        let compact_swap = (!view_same && !is_panel && view != View::Hidden && !resize)
            || (eyes_swap && !is_panel && view != View::Hidden);
        // A fade cut short goes on from the alpha it reached: new content
        // starts from it, and the same content keeps fading in.
        let fading = self.anim.fading(now);
        let fade_from = if is_panel || view == View::Hidden {
            None
        } else if compact_swap {
            Some(fading.unwrap_or(0.0))
        } else {
            fading
        };
        let hiding = view == View::Hidden;
        // What of the slot shows now, on the view drawn before this refresh.
        let slot_now = self.slot_drawn(now);
        if !hiding {
            self.drawn = view;
        }
        if retarget {
            // The slot's share goes from what shows to the target's; hiding
            // fades the island out as it is.
            self.slot_share = SlotShare {
                from: slot_now,
                to: if hiding { slot_now } else { slot_to },
            };
            let (shape, opacity) = self.anim.frame(now);
            // Drawn on screen before this morph (not hidden, not heading
            // for it): only then does a closing panel have content to fade.
            let from_visible = self.anim.to_opacity == 1.0;
            // Panel content caught mid-fade: a panel still opening, or a
            // closing one still fading out (as `draw` paints them).
            let panel_alpha = if !self.anim.running || !from_visible {
                None
            } else if was_panel.is_some() {
                Some(panel_fade(shape.h, self.anim.to.h, self.fade_floor_h))
            } else {
                self.leaving
                    .as_ref()
                    .map(|shown| panel_fade(shape.h, panel_rest_h(style, shown), self.fade_floor_h))
            };
            // The fill goes from what shows now to the view's own; from
            // nothing it simply is the view's.
            let fill_to = self.view_fill(&self.state.view());
            let fill_from = if from_visible {
                self.fill_at(now)
            } else {
                fill_to
            };
            self.fill = (fill_from, fill_to);
            // Hidden keeps the shape where it is and only fades it out.
            let (to, to_opacity) = target.map_or((shape, 0.0), |s| (s, 1.0));
            // The eyes alone (the view unchanged) morph slower and in and out;
            // a view change at the same time keeps the view's own rules.
            let eyes_only = eyes_swap && view_same && !hiding;
            let (ms, curve) = morph_timing(eyes_only, resize, !self.animations);
            self.anim = Anim {
                from: shape,
                from_opacity: opacity,
                to,
                to_opacity,
                start: now,
                ms,
                running: true,
                curve,
                fade_from,
            };
            (self.leaving, self.closed_panel) = leaving_on_morph(
                MorphTo {
                    is_panel,
                    hiding,
                    compact_swap,
                },
                was_panel,
                from_visible,
                self.leaving.take(),
                self.closed_panel.take(),
            );
            if !hiding {
                // The panel whose content fades in this morph: the one it
                // opens into, else the one closing.
                let rest_h = if is_panel {
                    to.h
                } else {
                    self.leaving
                        .as_ref()
                        .map_or(to.h, |shown| panel_rest_h(style, shown))
                };
                self.fade_floor_h = fade_floor(
                    is_panel,
                    to.h,
                    rest_h,
                    FadeStart {
                        shape_h: shape.h,
                        from_panel: was_panel.is_some(),
                        from_visible,
                        carried: self.fade_floor_h,
                        alpha: panel_alpha,
                        compact_h: notch.h,
                    },
                );
            }
            // The enlarged panel needs the taller canvas before it grows.
            self.sync_window_height(now);
            self.eyes_morph |= eyes_swap;
            if eyes_swap && view_same && !moves && !hiding {
                // Motion Saver, Off or Energy Saver: no morph, the island is
                // on the new shape at once.
                let (to, opacity) = (self.anim.to, self.anim.to_opacity);
                self.anim = Anim::settled(to, opacity);
                self.fill.0 = self.fill.1;
                self.eyes_morph = false;
                self.draw(now);
            } else {
                self.frame_log.start(now);
                self.ensure_clock();
            }
        } else if !self.animating() {
            // Same shape, but the content (track, notice text) may have changed.
            self.draw(now);
        }
        // The slot's logo has nowhere to fade once the island is back.
        if self.state.toast_slot().is_none() && !self.anim.running {
            self.slot_keep = None;
        }
        self.arm_deadline(now);
        self.sync_timers(now);
        // The greeting notice just ended: the greeting moment.
        if greeting_ended(&mut self.greeting_up, &self.state.view()) {
            self.play_when_settled(Moment::Greeting, now);
        }
        self.sync_lyrics_card(now);
        // The caption card's colours, font and language; nothing while
        // captions are off, and nothing sent when nothing changed.
        self.captions_card_sync();
        self.sync_settings();
        self.tip_check();
    }

    fn animating(&self) -> bool {
        self.animating_at(Instant::now())
    }

    /// The ✕'s fade target now: shown while the pointer is on the island this
    /// visit (hover tracking is armed), the notice is closable and in its full
    /// form, and the notch is not shrunk to the eyes.
    fn close_target(&self) -> f32 {
        let on =
            !self.hover.needs_arming() && self.state.notice_close_shown() && !self.peek.is_shrunk();
        if on { 1.0 } else { 0.0 }
    }

    /// Aims the ✕'s fade at its target when that changed, starting the frame
    /// clock only if the fade runs (no other timer). True when it changed; the
    /// caller makes sure a frame is drawn.
    fn sync_close_fade(&mut self, now: Instant) -> bool {
        let to = self.close_target();
        if self.close_fade.to == to {
            return false;
        }
        let ms = close_fade_ms(
            self.animations,
            self.config.robot_animation,
            self.bata.energy_saver,
        );
        self.close_fade.retarget(to, now, ms);
        if self.close_fade.moving(now) {
            self.ensure_clock();
        }
        true
    }

    /// What the renderer gets for the ✕ at `now`: `None` while it is fully
    /// faded out (nothing painted, no click region).
    fn notice_close_view(&self, now: Instant) -> Option<NoticeCloseView> {
        let alpha = self.close_fade.alpha(now);
        (alpha > 0.0).then_some(NoticeCloseView {
            alpha,
            hot: self.close_hot,
        })
    }

    /// The System page shows and a figure on it is still gliding.
    fn system_easing(&self, now: Instant) -> bool {
        self.system.ease.running(now)
            && system_sample_wanted(&self.state.view(), self.state.cleaner().is_some())
    }

    /// A morph, page slide or fade, sub-view reveal, lyric crossfade or
    /// scroll, robot lift/bounce, or the island's rise from behind a window
    /// runs at `now`.
    fn animating_at(&self, now: Instant) -> bool {
        self.anim.running
            || self.rise.is_some_and(|r| r.running(now))
            || self.system_easing(now)
            || self.slide.is_some()
            || self.reveal.is_some()
            || self.strip_motion.is_some()
            || self.close_fade.moving(now)
            || self.page_fade.is_some()
            || self.lyric_fading
            || self.lyric_scrolling
            || self.robot_moving(now)
            || self.look.moving()
            || self.glance.moving()
            || self.eyes_look.moving()
            || self.shelf.motion.running(now)
            || self.dock.moving()
            || self.widgets_moving(now)
            || self.play.is_some()
    }

    /// The window is `TALL_WINDOW_H` tall while the enlarged CLI panel shows
    /// or morphs, `WINDOW_H` otherwise (fewer pixels to push each frame).
    /// True when it changed.
    fn sync_window_height(&mut self, now: Instant) -> bool {
        let small = shape_for(self.config.style, ShapeKind::Panel);
        let tall = |s: ShapeRect| s.y + s.h > small.y + small.h + 1.0;
        let need = tall(self.anim.to)
            || (self.anim.running && (tall(self.anim.from) || tall(self.anim.frame(now).0)));
        let changed = self.win.as_mut().is_some_and(|w| w.set_tall(need));
        if changed {
            self.renderer.invalidate();
        }
        changed
    }

    /// Looks up lyrics for the playing track (memory first, else the worker;
    /// only with the setting on and while it plays) and tells the reducer
    /// whether the current track has them.
    fn sync_lyrics(&mut self, now: Instant) {
        let on = self.state.lyrics_on();
        let query = lyrics::track_query(self.state.media(), self.state.timeline());
        if on {
            let playing = self.state.media().is_some_and(|m| m.playing);
            if let Some(q) = self.lyrics.want(query.as_ref(), playing) {
                self.ask_lyrics(q);
            }
        }
        let ready = on && self.lyrics.ready(query.as_ref());
        if ready != self.state.lyrics_ready() {
            self.apply(Event::LyricsReady(ready), now);
        }
    }

    fn ask_lyrics(&mut self, q: Query) {
        let worker = self.lyrics_worker.get_or_insert_with(|| {
            lyrics::spawn_worker(
                self.tx.clone(),
                poster(self.hwnd, WM_APP_EVENT),
                DiskCache::new(DiskCache::default_dir()),
            )
        });
        if !worker.ask(q) {
            // The thread is gone; the next track starts another.
            self.lyrics_worker = None;
        }
    }

    /// Puts the line for the current position on screen (crossfading from
    /// the last one) and arms `TIMER_LYRIC` for the next change. Outside
    /// the lyric view the timer stops. True when the line changed.
    fn sync_lyric_line(&mut self, now: Instant) -> bool {
        let view = self.state.view();
        let (position, rate, playing, reach) = match (self.state.timeline(), self.state.media()) {
            (Some(t), Some(m)) if view == View::MediaLyric => (
                Some(lyrics::lyric_position(lyrics::position_at(
                    t, m.playing, now,
                ))),
                t.rate,
                m.playing,
                lyrics::lyric_reach(t.end_ms),
            ),
            _ => (None, 1.0, false, None),
        };
        let Some(pos) = position else {
            // Fading out (fullscreen) keeps its last line and width; leaving
            // the view forgets the width, so the next one starts fresh.
            if view != View::Hidden {
                self.lyrics.hide();
                self.lyric_fit = LyricFit::default();
            }
            self.set_lyric_timer(None);
            self.sync_lyric_scroll(now);
            return false;
        };
        let changed = self.lyrics.advance(pos, now);
        if changed || self.lyric_fit.width.is_none() {
            self.fit_lyric_line(now);
        }
        // The shrunk notch shows no lyric: the line still advances, but
        // its crossfade draws nothing. (A fade that ended is let go by the
        // frame tick, after its settled frame.)
        if !lyric_moves(&view, self.eyes) {
            self.lyric_fading = false;
        } else if self.lyrics.fading(now) {
            self.lyric_fading = true;
            self.ensure_clock();
        }
        // Paused, nothing changes until it plays (the next refresh re-arms);
        // at the track's end, or before a line the end cuts off, never.
        let wait = playing
            .then(|| self.lyrics.wait_ms(pos, rate, reach))
            .flatten();
        self.set_lyric_timer(wait);
        self.sync_lyric_scroll(now);
        changed
    }

    /// The widest lyric notch on this screen.
    fn lyric_max_w(&self) -> f32 {
        // Never below the lyric notch's minimum, which follows the user's width.
        self.win
            .as_ref()
            .map_or(LYRIC_MAX_W, |w| lyric_max_w(w.screen_w()))
            .max(self.notch_size().w)
    }

    /// The closed island's size as set in Settings.
    fn notch_size(&self) -> NotchSize {
        NotchSize::from_config(&self.config)
    }

    /// The fill alpha `view` is drawn with at rest: solid for the open panel,
    /// the configured one for the closed island and notices.
    fn view_fill(&self, view: &View) -> f32 {
        match view {
            View::Panel(_) => 1.0,
            _ => notch_fill::fill_alpha(self.config.notch_fill, self.config.notch_opacity),
        }
    }

    /// The surface fill's alpha at `now`: blended with the morph's eased
    /// progress while it runs (`notch_fill::morph_fill`).
    fn fill_at(&self, now: Instant) -> f32 {
        if !self.anim.running {
            return self.fill.1;
        }
        let t = self.anim.curve.ease(self.anim.progress(now));
        notch_fill::morph_fill(self.fill.0, self.fill.1, t)
    }

    /// Re-reads where the fill is heading after the setting changed: at rest
    /// it is there at once, during a morph it carries on toward it.
    fn sync_fill(&mut self) {
        let to = self.view_fill(&self.state.view());
        self.fill.1 = to;
        if !self.anim.running {
            self.fill.0 = to;
        }
    }

    /// A new notch size (the sliders, Reset): the closed island follows it.
    /// `live` (a slider being dragged) settles on the new shape at once so it
    /// tracks the pointer; otherwise the island morphs there. Nothing is
    /// saved here (the caller saves on commit).
    fn set_notch_size(&mut self, w: u32, h: u32, live: bool) {
        let size = NotchSize::from_px(w, h);
        self.config.notch_width = size.w as u32;
        self.config.notch_height = size.h as u32;
        let now = Instant::now();
        self.fade_floor_h = size.h;
        let view = self.state.view();
        if view == View::Hidden {
            // Nothing shows: rest on the new shape so the next appearance grows from it.
            let rest = shape_sized(self.config.style, size, ShapeKind::Idle);
            self.anim = Anim::settled(rest, 0.0);
            self.draw(now);
        } else if live && !matches!(view, View::Panel(_)) {
            self.refresh(now);
            // Settle where `refresh` was heading, so the shape follows the pointer.
            if self.anim.running {
                let (to, opacity) = (self.anim.to, self.anim.to_opacity);
                self.anim = Anim::settled(to, opacity);
                self.fill.0 = self.fill.1;
                self.draw(now);
            }
        } else {
            self.refresh(now);
        }
        self.sync_settings();
    }

    /// The see-through fill's opacity (0..=100 %); drawn at once.
    fn set_notch_opacity(&mut self, pct: u8) {
        self.config.notch_opacity = pct.min(100);
        self.on_fill_changed();
    }

    fn on_fill_changed(&mut self) {
        self.sync_fill();
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// Measures the line now on screen (as the renderer draws it) and works
    /// out the notch's width for it; `refresh` morphs the shape there.
    fn fit_lyric_line(&mut self, now: Instant) {
        let text_w = self
            .lyrics
            .shown(now)
            .and_then(|line| self.renderer.lyric_text_width(line.text));
        let max_w = self.lyric_max_w();
        self.lyric_fit.set_min(self.notch_size().w);
        self.lyric_fit.line(text_w, now, self.config.robot, max_w);
    }

    /// A line too long for the notch: one one-shot to when it starts
    /// scrolling, then the frame clock until it reaches its end. Nothing
    /// for a line that fits, outside the lyric view or once it has ended.
    fn sync_lyric_scroll(&mut self, now: Instant) {
        let shows = lyric_moves(&self.state.view(), self.eyes);
        let span = shows
            .then(|| self.lyric_fit.scroll_span())
            .flatten()
            .filter(|&(_, end)| end > now);
        let wait = match span {
            Some((start, _)) if start > now => {
                self.lyric_scrolling = false;
                Some(start - now)
            }
            Some(_) => {
                self.lyric_scrolling = true;
                self.ensure_clock();
                None
            }
            None => {
                self.lyric_scrolling = false;
                None
            }
        };
        unsafe {
            match wait {
                // `SetTimer` on the same id replaces the pending one.
                Some(wait) => {
                    let ms = (wait + Duration::from_millis(1))
                        .as_millis()
                        .min(u32::MAX as u128) as u32;
                    SetTimer(Some(self.hwnd), TIMER_LYRIC_SCROLL, ms, None);
                }
                None if self.lyric_scroll_timer => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LYRIC_SCROLL);
                }
                None => {}
            }
        }
        self.lyric_scroll_timer = wait.is_some();
    }

    /// One-shot: `SetTimer` on the same id replaces the pending one.
    fn set_lyric_timer(&mut self, wait_ms: Option<u64>) {
        unsafe {
            match wait_ms {
                Some(ms) => {
                    let ms = u32::try_from(ms).unwrap_or(u32::MAX);
                    SetTimer(Some(self.hwnd), TIMER_LYRIC, ms, None);
                }
                None if self.lyric_timer => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LYRIC);
                }
                None => {}
            }
        }
        self.lyric_timer = wait_ms.is_some();
    }

    fn ensure_clock(&mut self) {
        if self.clock.is_none() {
            self.clock = Some(FrameClock::start(self.hwnd.0 as isize));
        }
    }

    /// Runs the equalizer and countdown timers only while what they animate
    /// is on screen and moving.
    fn sync_timers(&mut self, now: Instant) {
        let view = self.state.view();
        self.sync_eq();
        let clock = clock_timer_wanted(&view, self.config.idle_clock);
        if clock != self.clock_timer {
            self.clock_timer = clock;
            if clock {
                self.clock_hour = Local::now().hour();
                self.arm_clock();
            } else {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_CLOCK);
                }
            }
        }
        let shows_countdown = matches!(view, View::PomodoroCompact | View::Panel(Page::Pomodoro));
        let running = matches!(self.state.pomodoro().status(), PomoStatus::Running { .. });
        let pomo = shows_countdown && running;
        if pomo != self.pomo_timer {
            self.pomo_timer = pomo;
            if pomo {
                self.arm_pomo(now);
            } else {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_POMO);
                }
            }
        }
        self.sync_rec_timers(now);
        self.sync_progress_timer(now);
        self.sync_robot_timers(now);
        self.sync_system_page();
        self.sync_mixer();
        self.sync_shelf();
        self.sync_phone_timer(now);
        self.sync_devices();
        self.sync_arrange();
        self.sync_notif();
        self.sync_outside_timer();
        self.sync_behind(now);
        self.sync_halo_timer();
        self.sync_pointer_watch(now);
        let cli = view == View::Panel(Page::Cli) && self.cli_clock.state().running();
        if cli != self.cli_timer {
            self.cli_timer = cli;
            unsafe {
                if cli {
                    SetTimer(Some(self.hwnd), TIMER_CLI, CLI_MS, None);
                } else {
                    let _ = KillTimer(Some(self.hwnd), TIMER_CLI);
                }
            }
        }
    }

    /// `TIMER_EQ` runs only while the compact view shows playing media at a
    /// screen somebody can see: not shrunk to the eyes, not fullscreen, not
    /// locked or with the display off. The lyric view joins it while the cat
    /// dances there. The capture thread behind the bars runs exactly while
    /// that timer does and the setting is on.
    fn sync_eq(&mut self) {
        let playing = self.state.media().is_some_and(|m| m.playing);
        let view = self.state.view();
        let dance_wanted = self.config.robot
            && self.config.cat_dance
            && self.motion().moves()
            && self.animations;
        let eq = eq_timer_wanted(
            view == View::MediaCompact,
            view == View::MediaLyric,
            dance_wanted,
            playing,
            self.eyes,
            self.fullscreen,
            self.screen_away(),
        );
        if eq != self.eq_timer {
            self.eq_timer = eq;
            unsafe {
                if eq {
                    // A dance starts at rest, the note at its place.
                    self.dance_anchor = Instant::now();
                    SetTimer(Some(self.hwnd), TIMER_EQ, EQ_MS, None);
                } else {
                    let _ = KillTimer(Some(self.hwnd), TIMER_EQ);
                }
            }
        }
        let capture = eq_capture_wanted(self.eq_timer, self.config.eq_follow_audio);
        if capture != self.eq_capture_on {
            self.eq_capture_on = capture;
            // Lazy: an island that never shows playing media never spawns it.
            if capture && self.eq_capture.is_none() {
                self.eq_capture = EqCapture::spawn();
            }
            if let Some(worker) = &self.eq_capture {
                worker.set_running(capture);
            }
        }
    }

    /// The equalizer bar heights from the real sound, or `None` for the
    /// sine: capture not wanted, or no device open (yet).
    fn eq_heights(&self) -> Option<[f32; crate::ui::geometry::BAR_COUNT]> {
        if !eq_capture_wanted(self.eq_timer, self.config.eq_follow_audio) {
            return None;
        }
        let shared = self.eq_capture.as_ref()?.shared();
        shared.live.load(Ordering::Relaxed).then(|| {
            crate::ui::geometry::audio_bar_heights(
                shared.levels(),
                shared.muted.load(Ordering::Relaxed),
            )
        })
    }

    /// The bass band (the lowest of the five) from the real sound, for the
    /// dancing cat: 0 while the output is muted, `None` where the bars use
    /// the sine, that is whenever `eq_heights` would.
    fn eq_bass(&self) -> Option<f32> {
        if !eq_capture_wanted(self.eq_timer, self.config.eq_follow_audio) {
            return None;
        }
        let shared = self.eq_capture.as_ref()?.shared();
        shared.live.load(Ordering::Relaxed).then(|| {
            if shared.muted.load(Ordering::Relaxed) {
                0.0
            } else {
                shared.levels()[0]
            }
        })
    }

    /// "Di belakang jendela": the poll runs only while the setting is on and
    /// the screen is not fullscreen; off (or fullscreen) the island is back
    /// above every window and nothing is read. A view that must be seen
    /// raises it at once, without waiting for the poll.
    fn sync_behind(&mut self, now: Instant) {
        let wanted = self.config.stay_behind && !self.fullscreen && !self.screen_away();
        if !wanted {
            if self.behind_ms != 0 {
                self.pace(TIMER_BEHIND, 0);
                self.behind.reset();
                self.cover_cache.clear();
                self.set_front(true);
                // No half-faded island is left behind (the clock, if it
                // runs, draws the settled frame; else draw it here).
                if self.rise.take().is_some() && !self.animating() {
                    self.draw(now);
                }
            }
            return;
        }
        // `follow_behind` sets the pace from what it reads.
        if self.behind_ms == 0 {
            self.pace(TIMER_BEHIND, behind::POLL_MS);
        }
        self.follow_behind(now);
    }

    /// The session is locked or the display is off: nobody is at the
    /// pointer, so the pointer polls pause.
    fn screen_away(&self) -> bool {
        self.session_locked || self.display_off
    }

    /// Locking, unlocking, the display going off or on: the pointer polls
    /// stop or start again.
    fn sync_pointer_polls(&mut self) {
        // Display on and unlock: whatever the screen kept is not trusted.
        self.renderer.invalidate();
        self.notif_sync_away();
        self.sync_outside_timer();
        self.sync_behind(Instant::now());
        self.sync_halo_timer();
        self.sync_eq();
        // A lock or a display change stops the paws and starts them again.
        self.sync_typing_timer(Instant::now());
        self.sync_mascot_timer(Instant::now());
        // The REC clock and meters stop too, and come back on unlock.
        self.sync_rec_timers(Instant::now());
        self.follow_pointer_settings(Instant::now());
        // The sleeping cat's wake poll stops while nobody is at the screen.
        // Back again, input made meanwhile wakes the cat at once.
        let now = Instant::now();
        if !self.screen_away() {
            self.fold_pc_input(now);
            let face = self.robot_frame(&self.state.view(), now).map(|r| r.expr);
            if face != self.bata.brain.shown() {
                self.refresh(now);
            }
        }
        self.sync_robot_timers(now);
    }

    /// Something the notch peek and the cat's look depend on changed (the
    /// session locked, a setting, the energy saver): the peek ends if it may
    /// not go on, the look goes back to the centre if the cat cannot follow,
    /// and the shared poll starts, re-paces or stops.
    fn follow_pointer_settings(&mut self, now: Instant) {
        if self.peek_guard() {
            // The notch comes back; `refresh` also syncs the look and the poll.
            self.refresh(now);
        } else {
            let looked = (
                self.look.value(),
                self.eyes_look.offset_dip(),
                self.glance.value(),
            );
            self.sync_look(now);
            self.sync_eyes_look();
            if (
                self.look.value(),
                self.eyes_look.offset_dip(),
                self.glance.value(),
            ) != looked
                && !self.animating()
            {
                // The cat was snapped back to the centre: draw it there.
                self.draw(now);
            }
            // The paws follow the same settings (the cat, its motion).
            self.sync_typing_timer(now);
            self.sync_mascot_timer(now);
            self.sync_pointer_watch(now);
        }
    }

    /// Sets repeating timer `id` to `ms` (0 stops it), only when its interval
    /// changes. Windows may coalesce it with other wake-ups within a small
    /// slack (`timer_slack`).
    fn pace(&mut self, id: usize, ms: u32) {
        let current = match id {
            TIMER_HALO => &mut self.halo_ms,
            TIMER_OUTSIDE => &mut self.outside_ms,
            TIMER_BEHIND => &mut self.behind_ms,
            TIMER_POINTER => &mut self.pointer_ms,
            _ => return,
        };
        if *current == ms {
            return;
        }
        *current = ms;
        unsafe {
            if ms == 0 {
                let _ = KillTimer(Some(self.hwnd), id);
            } else {
                SetCoalescableTimer(Some(self.hwnd), id, ms, None, timer_slack(ms));
            }
        }
    }

    /// The drag margin may be offered at all: the island is up in a view
    /// that is not a panel, the screen is not locked, off or fullscreen, and
    /// the island is not lowered behind windows.
    fn halo_allowed(&self) -> bool {
        let hidden_behind = self.config.stay_behind && self.behind.z() == behind::Z::Bottom;
        !self.fullscreen
            && !self.screen_away()
            && !hidden_behind
            // The eyes take a press that only brings the notch back.
            && !self.peek.is_shrunk()
            && !matches!(self.state.view(), View::Panel(_) | View::Hidden)
    }

    /// A trigger for the margin's poll: the pointer moved over the island,
    /// or a drag entered it. Cheap; the poll itself decides when to stop.
    fn arm_halo(&mut self) {
        self.halo_armed = Some(Instant::now());
        self.sync_halo_timer();
    }

    /// The margin's poll runs only after a trigger and while it is allowed;
    /// otherwise the margin is gone and nothing is read. A panel over the
    /// island pauses an armed poll (it resumes when the panel closes, and its
    /// first reading decides whether to go on).
    fn sync_halo_timer(&mut self) {
        let wanted = self.halo_armed.is_some() && self.halo_allowed();
        if wanted == (self.halo_ms != 0) {
            return;
        }
        // It starts slow; the first reading speeds it up for a drag.
        self.pace(TIMER_HALO, if wanted { halo::SLOW_MS } else { 0 });
        if !wanted {
            self.set_halo(false);
        }
    }

    /// `TIMER_HALO`: the margin follows "button down, pointer near"; the poll
    /// stops when neither can start a drag any more (`halo::keep_polling`),
    /// and runs fast only while a drag is close (`halo::poll_ms`).
    fn on_halo_poll(&mut self) {
        if self.halo_ms == 0 {
            return;
        }
        let down = left_button_down();
        let (mut near, mut close) = (false, false);
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_ok()
            && let (Some(island), Some(win)) = (self.renderer.island_px(), self.win.as_ref())
        {
            let (ox, oy) = win.screen_origin();
            let island = (
                island.0 + ox as f32,
                island.1 + oy as f32,
                island.2 + ox as f32,
                island.3 + oy as f32,
            );
            let cursor = (pt.x as f32, pt.y as f32);
            let near_px = halo::NEAR_DIP * win.scale();
            near = halo::is_near(cursor, island, near_px);
            close = halo::is_near(cursor, island, near_px * halo::FAST_REACH);
        }
        let since = self.halo_armed.map_or(Duration::MAX, |at| at.elapsed());
        if !self.halo_allowed() || !halo::keep_polling(down, near, since) {
            self.halo_armed = None;
            self.sync_halo_timer();
            return;
        }
        self.pace(TIMER_HALO, halo::poll_ms(down, close));
        self.set_halo(down && near);
    }

    fn set_halo(&mut self, on: bool) {
        if on == self.halo_on {
            return;
        }
        self.halo_on = on;
        self.renderer.set_halo(on);
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// `TIMER_BEHIND`.
    fn on_behind_timer(&mut self) {
        if self.behind_ms != 0 {
            self.follow_behind(Instant::now());
        }
    }

    /// Reads the pointer and puts the island where `behind` says; the poll
    /// slows down while the pointer is far from the top edge and nothing is
    /// about to change (`behind::poll_ms`).
    fn follow_behind(&mut self, now: Instant) {
        let mut pt = POINT::default();
        let have = unsafe { GetCursorPos(&mut pt).is_ok() };
        let near_top = !have
            || match (self.win.as_ref(), self.notch_span()) {
                (Some(win), Some(span)) => crate::pointer_watch::near_top(
                    pt.x,
                    pt.y - win.screen_origin().1,
                    span,
                    win.scale(),
                ),
                _ => true,
            };
        let on_strip = have && self.on_raise_strip(pt);
        let over_island = have && {
            let mut client = pt;
            unsafe { ScreenToClient(self.hwnd, &mut client) }.as_bool()
                && self
                    .renderer
                    .hit_test(client.x as f32, client.y as f32)
                    .is_some()
        };
        let inputs = behind::Inputs {
            needs_front: behind::view_needs_front(&self.state.view()),
            on_strip,
            over_island,
        };
        let was = self.behind.z();
        let z = self.behind.decide(inputs, now);
        if z != was {
            // Up or down: whether a window covers the cat is read afresh.
            self.cover_cache.clear();
        }
        // Coming up from the bottom behind a window: read "covered" now,
        // while it is still down, and draw the first (invisible) frame
        // before the island is raised so it does not pop for a frame.
        // `rise_wanted` is cheap; the covered read only runs for a raise.
        if was == behind::Z::Bottom && z == behind::Z::Top {
            if behind::rise_wanted(was, z, self.island_covered()) {
                self.rise = Some(behind::Rise::new(now, self.motion().moves()));
                self.draw(now);
                self.ensure_clock();
            }
        } else if z == behind::Z::Bottom {
            self.rise = None;
        }
        self.set_front(z == behind::Z::Top);
        if self.behind_ms != 0 {
            self.pace(
                TIMER_BEHIND,
                behind::poll_ms(self.behind.lower_pending(), near_top),
            );
        }
    }

    /// The screen point is on the strip along the top edge of the screen over
    /// the drawn island (plus a margin), not over the whole window.
    fn on_raise_strip(&self, pt: POINT) -> bool {
        let (Some(win), Some((left, _, right, _))) = (self.win.as_ref(), self.renderer.island_px())
        else {
            return false;
        };
        let (ox, oy) = win.screen_origin();
        let margin = behind::STRIP_MARGIN_DIP * win.scale();
        let span = (
            (ox as f32 + left - margin) as i32,
            (ox as f32 + right + margin) as i32,
        );
        behind::on_strip(oy, span, (pt.x, pt.y))
    }

    /// Another window is over the middle of the drawn island: the one at that
    /// screen point is not the island's own. (Half-covered with a free centre
    /// counts as not covered; the edges then pop.) A point over nothing, or
    /// an island not drawn, is not covered.
    fn island_covered(&self) -> bool {
        let (Some(win), Some((left, top, right, bottom))) =
            (self.win.as_ref(), self.renderer.island_px())
        else {
            return false;
        };
        let (ox, oy) = win.screen_origin();
        let centre = POINT {
            x: ox + ((left + right) / 2.0) as i32,
            y: oy + ((top + bottom) / 2.0) as i32,
        };
        let hit = unsafe { WindowFromPoint(centre) };
        !hit.is_invalid() && unsafe { GetAncestor(hit, GA_ROOT) } != self.hwnd
    }

    fn set_front(&mut self, front: bool) {
        let sank = self.win.as_mut().is_some_and(|win| win.set_front(front)) && !front;
        // Sunk behind the windows: a tip would float over what covers it.
        if sank {
            self.tip_hide();
        }
    }

    // ---- The shared pointer poll: the notch peek and the cat's look ----

    /// The notch may shrink to the eyes: "Di belakang jendela" is off (that
    /// mode has its own way of getting out of the way), nothing is fullscreen,
    /// the screen is on, and the view is a quiet compact one.
    fn peek_eligible(&self) -> bool {
        pointer_watch::peek_allowed(self.config.notch_peek, self.config.stay_behind)
            && !self.fullscreen
            && !self.screen_away()
            && self.win.is_some()
            && pointer_watch::peek_view(&self.state.view())
            // The logo of a busy toast has to stay on screen.
            && self.state.toast_slot().is_none()
    }

    /// Ends a peek that may not go on (the notch is whole again at once).
    /// True when it was shrunk or restored: the shape needs retargeting.
    fn peek_guard(&mut self) -> bool {
        if self.peek_eligible() {
            return false;
        }
        self.under_cache.clear();
        self.peek_last_pt = None;
        self.peek.reset()
    }

    /// The cat follows the pointer: it is on, motion is Full (after Energy
    /// Saver), the idle view shows it with a face that looks (`Expr::looks`:
    /// not asleep, not thinking), and the notch is not shrunk. In the compact
    /// music, lyric and timer views its slot is tight, so it stays still there.
    fn look_eligible(&self, now: Instant) -> bool {
        let view = self.state.view();
        pointer_watch::look_on(
            pointer_watch::look_allowed(self.config.robot, self.config.cat_follow_pointer),
            self.motion().moves(),
            view == View::Idle,
            self.peek.is_shrunk(),
            self.fullscreen,
            self.screen_away(),
            self.robot_frame(&view, now).map(|frame| frame.expr),
        )
    }

    /// The idle shape at the user's size: grown on the left for the typing
    /// cat's laptop while the cat is on and `typing_wanted` (the hover's Happy
    /// face does not count), else the plain one.
    fn idle_shape(&self, now: Instant) -> ShapeRect {
        let (style, robot, notch) = (self.config.style, self.config.robot, self.notch_size());
        if self.typing_shape(now) {
            idle_shape_typing(style, notch, robot)
        } else {
            shape_with_robot_sized(style, notch, ShapeKind::Idle, robot)
        }
    }

    /// The idle island is (or would be) grown for the typing cat's laptop:
    /// the cat is on and `typing_wanted`.
    fn typing_shape(&self, now: Instant) -> bool {
        let inputs = self
            .bata
            .brain
            .inputs(Spot::Idle, now, self.sleep_after(), self.motion());
        self.config.robot && robot::typing_wanted(&inputs)
    }

    /// The closed view's notch as it is without the peek, in logical px;
    /// `None` for a view that never shrinks.
    fn normal_shape(&self, view: &View) -> Option<ShapeRect> {
        let (style, robot, notch) = (self.config.style, self.config.robot, self.notch_size());
        let kind = match view {
            // The typing cat's island is wider on the left.
            View::Idle => return Some(self.idle_shape(Instant::now())),
            View::MediaCompact => ShapeKind::MediaCompact,
            View::MediaLyric => {
                return Some(lyric_shape_sized(style, notch, self.lyric_fit.width()));
            }
            View::PomodoroCompact => ShapeKind::PomodoroCompact,
            _ => return None,
        };
        Some(shape_with_robot_sized(style, notch, kind, robot))
    }

    /// The shrunk notch's eyes follow the pointer: it is shrunk, "Ikuti
    /// kursor" is on, and the peek may go on (not fullscreen, not locked).
    /// The cat is not asked: the eyes show with it off.
    fn eyes_look_eligible(&self) -> bool {
        self.peek.is_shrunk() && self.config.cat_follow_pointer && self.peek_eligible()
    }

    /// How the eyes follow now (`pointer_watch::eyes_motion`).
    fn eyes_motion(&self) -> EyesMotion {
        pointer_watch::eyes_motion(
            self.config.cat_follow_pointer,
            self.motion().moves(),
            self.animations,
        )
    }

    /// The eyes cannot look any more (not shrunk, "Ikuti kursor" off,
    /// fullscreen, locked): they are centred at once, with no ease. And a
    /// motion setting that stopped easing finishes a running ease where it
    /// was heading.
    fn sync_eyes_look(&mut self) {
        if !self.eyes_look_eligible() {
            self.eyes_look.snap_to_centre();
            self.eyes_look_at = None;
        } else if self.eyes_motion() != EyesMotion::Ease && self.eyes_look.finish() {
            self.eyes_look_at = None;
        }
    }

    /// One poll reading for the eyes: the pointer, in island dip, sets the
    /// look's target. Eased on the frame clock with Motion Full and Windows
    /// animations on; otherwise the eyes jump there and are drawn once.
    fn eyes_look_tick(&mut self, pt: POINT, now: Instant) {
        let view = self.state.view();
        let Some(normal) = self.normal_shape(&view) else {
            return;
        };
        let Some((origin, k)) = self
            .win
            .as_ref()
            .map(|win| (win.screen_origin(), win.scale()))
        else {
            return;
        };
        let (x, y) = ((pt.x - origin.0) as f32 / k, (pt.y - origin.1) as f32 / k);
        let pill = shape_sized(self.config.style, self.notch_size(), ShapeKind::Eyes);
        let half_span = normal.w / 2.0 + pointer_watch::PEEK_ZONE_DIP;
        let (vx, vy) = pointer_watch::eyes_target(
            x,
            y,
            pill.x + pill.w / 2.0,
            pill.y + pill.h / 2.0,
            half_span,
        );
        match self.eyes_motion() {
            EyesMotion::Still => {}
            EyesMotion::Jump => {
                if self.eyes_look.jump_to(vx, vy) && !self.animating() {
                    self.draw(now);
                }
            }
            EyesMotion::Ease => {
                if self.eyes_look.set_target(vx, vy) {
                    self.ensure_clock();
                }
            }
        }
    }

    /// One frame-clock tick of the eyes' look: eased to `now`.
    fn advance_eyes_look(&mut self, now: Instant) {
        if !self.eyes_look.moving() {
            self.eyes_look_at = None;
            return;
        }
        let dt = self
            .eyes_look_at
            .map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
        self.eyes_look.advance(dt);
        self.eyes_look_at = self.eyes_look.moving().then_some(now);
    }

    /// Windows' animation setting was read again: the eyes' ease and blink
    /// follow it at once (nothing runs unless the notch is shrunk).
    fn animations_changed(&mut self) {
        let on = client_animations_on();
        if on == self.animations {
            return;
        }
        self.animations = on;
        let now = Instant::now();
        if self.eyes {
            self.sync_eyes_look();
            self.sync_robot_timers(now);
        }
        self.refresh(now);
    }

    /// The cat cannot follow the pointer any more (`look_eligible` is false):
    /// its look is back at the centre at once, with no ease. Every way out
    /// leaves nothing to ease on screen: the cat is gone (off, another view,
    /// the eyes, fullscreen), nobody is looking (a locked session, a dark
    /// display), motion is not Full, or the face does not look and is drawn
    /// centred anyway (`Pose::looking_as`). Nothing to do while it follows.
    fn sync_look(&mut self, now: Instant) {
        if !self.look_eligible(now) {
            self.look.snap_to_centre();
            self.look_at = None;
            self.glance.snap_to_centre();
            self.glance_at = None;
            self.glance_rest.reset();
            return;
        }
        // The typing cat never turns its head, and the others never glance.
        let typing = self
            .robot_frame(&self.state.view(), now)
            .is_some_and(|frame| frame.expr.glances());
        if typing {
            self.look.snap_to_centre();
            self.look_at = None;
        } else {
            self.glance.snap_to_centre();
            self.glance_at = None;
            self.glance_rest.reset();
        }
    }

    /// The island is lowered behind windows ("Di belakang jendela") with one
    /// over it, so nobody sees the cat: its look stays where it is. The
    /// window read (`island_covered`) runs only while lowered, and at most
    /// once a `pointer_watch::UNDER_RECHECK`.
    fn look_hidden_behind(&mut self, now: Instant) -> bool {
        let lowered = self.config.stay_behind && self.behind.z() == behind::Z::Bottom;
        let mut cache = std::mem::take(&mut self.cover_cache);
        let hidden =
            pointer_watch::look_hidden(lowered, || cache.get(now, || self.island_covered()));
        self.cover_cache = cache;
        hidden
    }

    /// Where the typing cat's eyes are, vertically, in window dip: the middle
    /// of the idle island they sit in (the glance's vertical reference).
    fn glance_eyes_y(&self) -> f32 {
        let s = self.anim.to;
        s.y + s.h / 2.0
    }

    /// One frame-clock tick of the typing cat's glance: eased to `now`.
    fn advance_glance(&mut self, now: Instant) {
        if !self.glance.moving() {
            self.glance_at = None;
            return;
        }
        let dt = self
            .glance_at
            .map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
        self.glance.advance(dt);
        self.glance_at = self.glance.moving().then_some(now);
    }

    /// One frame-clock tick of the cat's look: eased to `now`; once it has
    /// arrived the clock may stop.
    fn advance_look(&mut self, now: Instant) {
        if !self.look.moving() {
            self.look_at = None;
            return;
        }
        let dt = self
            .look_at
            .map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
        self.look.advance(dt);
        self.look_at = self.look.moving().then_some(now);
    }

    /// Starts, re-paces or stops `TIMER_POINTER`: it runs only while the notch
    /// peek or the cat's look is eligible (which also rules out fullscreen,
    /// a locked session and a display that is off), and its pace follows
    /// `pointer_watch::poll_ms`.
    fn sync_pointer_watch(&mut self, now: Instant) {
        let follow = pointer_watch::follow_on(self.look_eligible(now), self.eyes_look_eligible());
        let peek = self.peek_eligible();
        let ms = pointer_watch::poll_ms(follow, peek, self.pointer_near);
        if ms == 0 {
            self.under_cache.clear();
            self.peek_last_pt = None;
            self.cover_cache.clear();
            // The next start reads at the quick pace first.
            self.pointer_near = true;
        }
        self.pace(TIMER_POINTER, ms);
    }

    /// `TIMER_POINTER`: one `GetCursorPos`, fed to both features.
    fn on_pointer_timer(&mut self) {
        if self.pointer_ms == 0 {
            return;
        }
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return;
        }
        let now = Instant::now();
        let Some((origin, scale, screen_w)) = self
            .win
            .as_ref()
            .map(|win| (win.screen_origin(), win.scale(), win.screen_w()))
        else {
            return;
        };
        self.pointer_near = pointer_watch::near_top(
            pt.x,
            pt.y - origin.1,
            pointer_watch::monitor_span(origin.0, WINDOW_W * scale, screen_w, scale),
            scale,
        );
        if self.peek_eligible() {
            self.peek_tick(pt, now);
        }
        if self.eyes_look_eligible() {
            self.eyes_look_tick(pt, now);
        }
        if self.look_eligible(now) && !self.look_hidden_behind(now) {
            // The island is centred on the monitor: so is the notch.
            let centre = origin.0 as f32 + WINDOW_W * scale / 2.0;
            let half = screen_w * scale / 2.0;
            let typing = self
                .robot_frame(&self.state.view(), now)
                .is_some_and(|frame| frame.expr.glances());
            if typing {
                // The typing cat glances with its eyes only, while the
                // pointer moves; once it has rested they go back to the
                // screen. The stillness rides these readings: no timer.
                let target = if self.glance_rest.reading((pt.x, pt.y), now) {
                    // The eyes' line, in dip like the pointer's y.
                    let eyes_y = self.glance_eyes_y();
                    pointer_watch::glance_target(
                        pt.x as f32,
                        (pt.y - origin.1) as f32 / scale,
                        centre,
                        centre - half,
                        centre + half,
                        eyes_y,
                    )
                } else {
                    (0.0, 0.0)
                };
                if self.glance.set_target(target.0, target.1) {
                    self.ensure_clock();
                }
            } else {
                let v =
                    pointer_watch::look_target(pt.x as f32, centre, centre - half, centre + half);
                if self.look.set_target(v) {
                    self.ensure_clock();
                }
            }
        }
        // Eligibility may have changed with the peek; so may the pace.
        self.sync_pointer_watch(now);
    }

    /// Feeds the pointer to the notch peek and applies what it decides.
    fn peek_tick(&mut self, pt: POINT, now: Instant) {
        let view = self.state.view();
        let Some(normal) = self.normal_shape(&view) else {
            return;
        };
        let Some((origin, k)) = self
            .win
            .as_ref()
            .map(|win| (win.screen_origin(), win.scale()))
        else {
            return;
        };
        let (x, y) = ((pt.x - origin.0) as f32 / k, (pt.y - origin.1) as f32 / k);
        let style = self.config.style;
        let pill = shape_sized(style, self.notch_size(), ShapeKind::Eyes);
        let in_zone = pointer_watch::peek_zone(normal).contains(x, y);
        let over_normal = island_contains(style, normal, x, y, AA_MARGIN);
        let over_pill = island_contains(style, pill, x, y, AA_MARGIN);
        let before = self.peek.state();
        let moved = self.peek_last_pt.is_some_and(|last| last != (pt.x, pt.y));
        if !in_zone {
            self.under_cache.clear();
        }
        // The walk over the windows only when a shrink is possible, once on
        // the approach and at most once a second while the pointer stays.
        // A pointer over the notch is read too when it just arrived from
        // outside (a fast move lands on it between two polls).
        let candidate =
            before == PeekState::Normal && in_zone && (!over_normal || self.peek.arriving());
        let window_under = candidate
            && self
                .under_cache
                .get(now, || window_under_notch(origin, k, normal));
        let inputs = PeekInputs {
            eligible: true,
            in_zone,
            over_normal,
            over_pill,
            window_under,
            dragging: in_zone && left_button_down(),
            moved,
        };
        self.peek_last_pt = Some((pt.x, pt.y));
        let after = self.peek.step(inputs, now);
        if std::mem::discriminant(&before) != std::mem::discriminant(&after) {
            self.on_peek_changed(before, after, now);
        }
    }

    /// The peek went from `from` to `to`: the island morphs to the eyes or
    /// back. A notch that comes back under a pointer that rests on it starts
    /// the hover as if the pointer had just arrived, so it opens as usual.
    fn on_peek_changed(&mut self, from: PeekState, to: PeekState, now: Instant) {
        // The drag margin is off while the eyes show.
        self.sync_halo_timer();
        let back = matches!(from, PeekState::Shrunk { .. }) && to == PeekState::Restored;
        if back && self.hover.needs_arming() {
            self.begin_hover();
        } else {
            self.refresh(now);
        }
    }

    /// A click on the eyes: the notch comes back at once (and that press does
    /// nothing else).
    fn restore_peek(&mut self) {
        let from = self.peek.state();
        if self.peek.restore() {
            self.on_peek_changed(from, self.peek.state(), Instant::now());
        }
    }

    /// Polls the mouse buttons only while the panel is kept open: a press
    /// already down when the poll starts is not a click outside.
    fn sync_outside_timer(&mut self) {
        let wanted = self.state.outside_click_wanted() && !self.screen_away();
        if wanted == (self.outside_ms != 0) {
            return;
        }
        if wanted {
            // Reading also spends the "pressed since" bits of old presses.
            self.outside_edge.reset(mouse_buttons().0);
        }
        self.pace(TIMER_OUTSIDE, if wanted { OUTSIDE_MS } else { 0 });
    }

    /// A button went down since the last poll (held, or a quick click that
    /// came and went between two polls): unless the island took that press
    /// itself (its own button-down message), it was outside and closes the
    /// panel. With no input for a while the poll slows down; the "pressed
    /// since" bit still catches a click between two slow polls.
    fn on_outside_poll(&mut self) {
        if self.outside_ms == 0 {
            return;
        }
        let (down, tapped) = mouse_buttons();
        if self.outside_edge.poll(down, tapped) {
            self.apply_now(Event::Dismiss);
        }
        if self.outside_ms != 0 {
            self.pace(TIMER_OUTSIDE, outside_poll_ms(input_idle_ms()));
        }
    }

    /// Re-armed on every sync (a seek or new track re-aligns it) and after
    /// each tick; killed as soon as the Music page, playback or the length
    /// goes away, so a paused or idle island runs no timer for it.
    fn sync_progress_timer(&mut self, now: Instant) {
        let playing = self.state.media().is_some_and(|m| m.playing);
        let wait = progress_wait(
            &self.state.view(),
            track_progress(&self.state, now),
            playing,
        );
        unsafe {
            match wait {
                // `SetTimer` on the same id replaces the pending one.
                Some(ms) => {
                    SetTimer(Some(self.hwnd), TIMER_PROGRESS, ms, None);
                }
                None if self.progress_timer => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_PROGRESS);
                }
                None => {}
            }
        }
        self.progress_timer = wait.is_some();
    }

    /// `TIMER_REC` and `TIMER_REC_LEVEL` run only while a recording shows and
    /// moves; they are killed the moment the view leaves the screen.
    fn sync_rec_timers(&mut self, now: Instant) {
        let view = self.state.view();
        let paused = self.state.rec().is_some_and(|r| r.paused());
        let recording = self.state.rec().is_some();
        let away = self.screen_away();
        let clock = rec_clock_timer_wanted(
            recording && matches!(view, View::RecordCompact | View::Panel(Page::Record)),
            paused,
            away,
        );
        if clock != self.rec_timer {
            self.rec_timer = clock;
            if clock {
                self.arm_rec(now);
            } else {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_REC);
                }
            }
        }
        let level = rec_level_timer_wanted(
            recording && view == View::RecordCompact,
            paused,
            self.motion().timed(),
            away,
        );
        if level != self.rec_level_timer {
            self.rec_level_timer = level;
            if level {
                self.arm_rec_level();
            } else {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_REC_LEVEL);
                }
            }
        }
    }

    /// One-shot `REC_LEVEL_MS` from now; Windows may fold it into another
    /// wake within the usual slack.
    fn arm_rec_level(&self) {
        unsafe {
            SetCoalescableTimer(
                Some(self.hwnd),
                TIMER_REC_LEVEL,
                REC_LEVEL_MS,
                None,
                timer_slack(REC_LEVEL_MS),
            );
        }
    }

    /// One-shot to the next whole second of the recorded time.
    fn arm_rec(&self, now: Instant) {
        let elapsed = self
            .state
            .rec()
            .map_or(Duration::ZERO, |r| r.elapsed(now));
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_REC, ms_to_next_whole_second(elapsed), None);
        }
    }

    /// One-shot, re-armed after each redraw, so the shown second ticks over on time.
    fn arm_pomo(&self, now: Instant) {
        let wait = ms_to_next_second(self.state.pomodoro().remaining(now));
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_POMO, wait, None);
        }
    }

    /// The wall clock jumped (time or zone changed, or the PC woke up): due
    /// reminders ring and the reminder timer re-arms for the new time; a
    /// showing clock redraws now and re-aligns to the new minute boundary.
    fn on_clock_jump(&mut self) {
        self.fire_reminders(false);
        // The sticky notes' reminders too: the missed ones fire, and the
        // one-shot is armed for the new time.
        self.sticky_remind_fire();
        // A running story ends; a jump over an hour is not the hour moment.
        self.play_clock_jumped = self.play.is_some();
        self.clock_hour = Local::now().hour();
        if !self.clock_timer {
            return;
        }
        if !self.animating() {
            self.draw(Instant::now());
        }
        self.arm_clock();
    }

    /// One-shot to just past the next minute boundary, re-armed after each redraw.
    fn arm_clock(&self) {
        let wait = ms_to_next_minute(Local::now().time());
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_CLOCK, wait, None);
        }
    }

    fn arm_deadline(&self, now: Instant) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_DEADLINE);
            if let Some(deadline) = self.state.next_deadline() {
                // Round up so the timer never fires before the notice expires.
                let wait = deadline.saturating_duration_since(now) + Duration::from_millis(1);
                let ms = wait.as_millis().min(u32::MAX as u128) as u32;
                SetTimer(Some(self.hwnd), TIMER_DEADLINE, ms, None);
            }
        }
    }

    fn on_timer(&mut self, id: usize) {
        match id {
            TIMER_SYSTEM => {
                self.check_fullscreen();
                self.check_lyrics_card_fullscreen();
                // Pinned notes hide over a fullscreen app; free when none are.
                self.widgets_fullscreen_tick();
                self.widgets_mood_tick();
                self.check_editor();
                // A focus lost without its message must not pin the panel.
                if self.state.cli_focused() && !self.cli_has_keyboard() {
                    self.cli_prev = None;
                    self.apply_now(Event::CliFocus(false));
                }
                if self.state.clip_focused() && !self.clip_has_keyboard() {
                    self.cli_prev = None;
                    self.apply_now(Event::ClipFocus(false));
                }
                if self.state.notif_focused() && !self.notif_has_keyboard() {
                    self.cli_prev = None;
                    self.apply_now(Event::NotifFocus(false));
                }
            }
            TIMER_NOTIF_SAVE => self.on_notif_timer(),
            TIMER_WIDGET_SAVE => self.on_widget_timer(),
            TIMER_STICKY_REMIND => self.sticky_remind_fire(),
            clip::TIMER_CLIP_RETRY
            | clip::TIMER_CLIP_SAVE
            | clip::TIMER_CLIP_HOLD
            | clip::TIMER_CLIP_DRAG => self.on_clip_timer(id),
            shot::TIMER_SHOT_RETRY => self.on_shot_timer(),
            shot::TIMER_SHOT_SHOW => self.on_shot_show_timer(),
            TIMER_BATTERY => self.check_battery(),
            TIMER_DEADLINE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_DEADLINE);
                }
                self.apply_now(Event::Tick);
            }
            // While an animation runs, the frame clock draws anyway.
            TIMER_EQ => {
                if !self.animating() {
                    self.draw_eq_tick(Instant::now());
                }
            }
            TIMER_POMO => {
                let now = Instant::now();
                if !self.animating() {
                    self.draw(now);
                }
                if self.pomo_timer {
                    self.arm_pomo(now);
                }
            }
            TIMER_SCREEN_COUNTDOWN => self.screen_countdown_tick(),
            TIMER_REC => {
                let now = Instant::now();
                if !self.animating() {
                    self.draw(now);
                }
                if self.rec_timer {
                    self.arm_rec(now);
                }
            }
            TIMER_REC_LEVEL => {
                // A one-shot: gone unless re-armed.
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_REC_LEVEL);
                }
                if !self.animating() {
                    self.draw(Instant::now());
                }
                if self.rec_level_timer {
                    self.arm_rec_level();
                }
            }
            TIMER_DAY => self.check_date(),
            TIMER_REMINDER => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_REMINDER);
                }
                // Also re-arms: the next one, or another hour's wait.
                self.fire_reminders(false);
            }
            TIMER_LYRIC => {
                let now = Instant::now();
                // A new line may want another width: refresh retargets the
                // shape, and the crossfade's frame clock draws it.
                if self.sync_lyric_line(now) {
                    self.refresh(now);
                }
            }
            TIMER_LYRIC_RETRY => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LYRIC_RETRY);
                }
                self.lyrics.retry_due();
                self.sync_lyrics(Instant::now());
            }
            TIMER_LYRIC_SCROLL => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LYRIC_SCROLL);
                }
                self.lyric_scroll_timer = false;
                // Starts the frame clock (or re-arms if it fired early).
                self.sync_lyric_scroll(Instant::now());
            }
            TIMER_CLOCK => {
                self.check_hour(Instant::now());
                if !self.animating() {
                    self.draw(Instant::now());
                }
                if self.clock_timer {
                    self.arm_clock();
                }
            }
            TIMER_PROGRESS => {
                let now = Instant::now();
                if !self.animating() {
                    self.draw(now);
                }
                self.sync_progress_timer(now);
            }
            TIMER_CLI => self.redraw_cli(),
            TIMER_SYSTEM_PAGE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_SYSTEM_PAGE);
                }
                self.system.page_timer = false;
                // Asks for the next sample while the page still shows.
                self.sync_system_page();
            }
            TIMER_SYSTEM_WARN => self.on_system_warn_timer(),
            TIMER_UPDATE => self.on_update_timer(),
            TIMER_TRANSCRIBE => self.transcribe_settle(),
            TIMER_CAPTION_LINE => self.on_caption_line_timer(),
            TIMER_MIXER => {
                if self.state.view() == View::Panel(Page::Mixer) {
                    self.mixer_scan();
                }
            }
            TIMER_VOLUME => self.on_volume_timer(),
            TIMER_DEVICES => self.on_devices_timer(),
            TIMER_SHELF_HOLD => self.on_shelf_hold_timer(),
            TIMER_PHONE => self.on_phone_timer(),
            TIMER_OUTSIDE => self.on_outside_poll(),
            TIMER_BEHIND => self.on_behind_timer(),
            TIMER_POINTER => self.on_pointer_timer(),
            TIMER_TYPING => self.on_typing_timer(),
            TIMER_MASCOT_PLAY => self.on_mascot_play_timer(),
            TIMER_HALO => self.on_halo_poll(),
            TIMER_BLINK => self.on_blink(),
            TIMER_ROBOT => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_ROBOT);
                }
                self.bata.change_timer = false;
                let now = Instant::now();
                self.fold_pc_input(now);
                let face = self.robot_frame(&self.state.view(), now).map(|r| r.expr);
                if robot::wake_tick_quiet(self.bata.brain.shown(), face) {
                    // A wake poll that finds the cat still asleep: re-arm
                    // only, nothing to draw.
                    self.sync_robot_timers(now);
                } else {
                    // The face may have changed (asleep, woken, done
                    // thinking); re-arms.
                    self.refresh(now);
                }
            }
            _ => {}
        }
    }

    /// One frame-clock tick: draws the shape morph and page slide at the current time.
    fn animate(&mut self) {
        let Some(clock) = &self.clock else {
            // A tick posted just before the clock stopped.
            return;
        };
        clock.ack();
        let now = Instant::now();
        // The cat's look eases to this frame before it is drawn, so the frame
        // that reaches the target is the last one the clock draws for it.
        self.advance_look(now);
        self.advance_glance(now);
        self.advance_eyes_look(now);
        self.dock_step(now);
        self.widgets_step(now);
        self.advance_play(now);
        let shape_done = self.anim.progress(now) >= 1.0;
        let slide_done = self.slide.is_none_or(|s| s.progress(now) >= 1.0);
        let reveal_done = self.reveal.is_none_or(|r| r.done(now));
        let strip_done = self.strip_motion.as_ref().is_none_or(|m| m.motion.done(now));
        let fade_done = self.page_fade.is_none_or(|f| f.progress(now) >= 1.0);
        // The resting t = 1 frame is always drawn before the clock stops, so a
        // fade-out never leaves faint pixels that still catch clicks, and the
        // settled page registers its click regions.
        self.draw(now);
        self.frame_log.frame(now);
        if shape_done && self.anim.running {
            self.anim.running = false;
            self.eyes_morph = false;
            // The panel that was closing is gone with its morph.
            self.leaving = None;
            if self.state.toast_slot().is_none() {
                self.slot_keep = None;
            }
            self.frame_log.finish();
            // Back from the enlarged panel: the canvas shrinks again.
            if self.sync_window_height(now) {
                self.draw(now);
            }
        }
        // Checked at the drawn frame's time, so its last frame was the settled one.
        if self.lyric_fading && !self.lyrics.fading(now) {
            self.lyric_fading = false;
        }
        // Likewise the scroll ends on the frame with the line at its end.
        if self.lyric_scrolling
            && self
                .lyric_fit
                .scroll_span()
                .is_none_or(|(_, end)| now >= end)
        {
            self.lyric_scrolling = false;
        }
        if slide_done {
            self.slide = None;
        }
        if reveal_done {
            self.reveal = None;
        }
        if strip_done {
            self.strip_motion = None;
        }
        if fade_done {
            self.page_fade = None;
        }
        // The rise ends on the frame drawn at its end: fully opaque, in place.
        if self.rise.is_some_and(|r| !r.running(now)) {
            self.rise = None;
        }
        // The shelf's cards likewise: the frame just drawn was their last.
        self.shelf.motion.prune(now);
        // At the drawn frame's time, so a lift or bounce ends on its rest frame.
        if !self.animating_at(now) {
            // Dropping joins the frame thread, which wakes within one frame.
            self.clock = None;
            // The blink waited for the animation to end.
            self.sync_robot_timers(now);
        }
    }

    /// Asks Windows whether a fullscreen app is in front and tells the state
    /// when that changed. While the Susun pick overlay is up the question is
    /// not asked: the overlay is Vale's own topmost window over the whole
    /// virtual screen and `SHQueryUserNotificationState` reports it as a
    /// fullscreen app, which would hide the island and cancel the pick a
    /// second after it began. The last known state stays, in `self.fullscreen`
    /// and in the state alike (they never disagree). A real fullscreen app
    /// takes the foreground from the overlay, which cancels the pick; the
    /// next check then sees it.
    fn check_fullscreen(&mut self) {
        let own_overlay_up = arrange_overlay::active();
        let reported = if own_overlay_up {
            self.fullscreen
        } else {
            // Per monitor: only a fullscreen app on the notch's monitor counts.
            is_fullscreen(Some(self.hwnd))
        };
        if let Some(fullscreen) = fullscreen_event(reported, own_overlay_up, self.fullscreen) {
            self.fullscreen = fullscreen;
            self.apply_now(Event::Fullscreen(fullscreen));
        }
    }

    /// The island's monitor as `[left, right)` in physical pixels, from the
    /// window's current position; `None` before the window exists.
    fn notch_span(&self) -> Option<(i32, i32)> {
        self.win.as_ref().map(|win| {
            crate::pointer_watch::monitor_span(
                win.screen_origin().0,
                WINDOW_W * win.scale(),
                win.screen_w(),
                win.scale(),
            )
        })
    }

    fn check_battery(&mut self) {
        let reading = read_battery();
        if let Some((percent, charging)) = reading
            && self.battery.update(percent, charging)
        {
            self.apply_now(Event::Notice(battery_notice(percent)));
        }
        // The idle robot looks low below the threshold; Energy Saver stills it.
        let low = battery_low_now(reading, self.config.battery_threshold);
        let saver = energy_saver_on();
        if low != self.bata.brain.battery_low || saver != self.bata.energy_saver {
            self.bata.brain.battery_low = low;
            self.bata.energy_saver = saver;
            self.refresh(Instant::now());
        }
        // Captions stop on battery and go on again on the charger.
        self.captions_power();
    }

    /// Arms hover and leave tracking when the pointer enters; the island
    /// expands only once `WM_MOUSEHOVER` reports a dwell.
    fn on_mouse_move(&mut self, lparam: LPARAM) {
        // The pointer over the island (or its margin) is the halo poll's
        // trigger: a drag may start from here. A no-op while it runs.
        if self.halo_allowed() {
            self.arm_halo();
        }
        // The user is at the mouse: a slowed outside-click poll speeds up.
        if self.outside_ms > OUTSIDE_MS {
            self.pace(TIMER_OUTSIDE, OUTSIDE_MS);
        }
        // A slider drag holds the capture: the pointer may be anywhere, and
        // leaving the island mid-drag is not leaving it (`on_release` checks).
        if self.mixer_drag.is_some() {
            self.mixer_follow(lparam);
            return;
        }
        // Shrunk to the eyes: they only restore the notch (by a click or by
        // resting on them). Nothing hovers, no panel dwell starts.
        if self.peek.is_shrunk() {
            return;
        }
        // A busy toast's own spot (its strip, its logo, the full toast it
        // peeks as) holds it and shows it whole; only a change is sent.
        let over = self.toast_pointer_at(lparam);
        self.set_toast_pointer(over);
        // A fast move onto the notch can reach the island before the poll has
        // read the pointer outside it: step the peek now, so a window under
        // the notch still shrinks it before the hover starts. Once per
        // approach (hover then no longer needs arming), no timer.
        if self.peek_eligible()
            && self.peek.state() == PeekState::Normal
            && self.hover.needs_arming()
        {
            let mut pt = POINT::default();
            if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                // This reading is the poll's too: a shrink re-paces the poll
                // (`sync_pointer_watch`), which must see the pointer near the
                // top now, not where the last, possibly slow, poll read it.
                if let (Some(win), Some(span)) = (self.win.as_ref(), self.notch_span()) {
                    self.pointer_near = pointer_watch::near_top(
                        pt.x,
                        pt.y - win.screen_origin().1,
                        span,
                        win.scale(),
                    );
                }
                self.peek_tick(pt, Instant::now());
                if self.peek.is_shrunk() {
                    return;
                }
            }
        }
        // The faint margin round a small island takes a drag (the shelf),
        // not the pointer: nothing hovers there.
        if !self.on_island(lparam) {
            // From the island onto its margin is leaving it (no leave
            // message comes while the pointer stays in the window).
            if !self.hover.needs_arming() {
                self.on_mouse_leave();
            }
            return;
        }
        self.dock_follow(lparam);
        self.track_close_hot(Some(lparam));
        let group = match self.hit_at(lparam) {
            Some(Hit::RecMuteCompact(source)) => Some(source),
            _ => None,
        };
        self.set_rec_mute_hot(group);
        if self.state.view() == View::Panel(Page::Arrange) {
            let hit = self.hit_at(lparam);
            self.arrange_hover(hit.as_ref());
        }
        if self.state.view() == View::Panel(Page::Clipboard) {
            let hit = self.hit_at(lparam);
            self.clip_hover(client_xy(lparam), hit.as_ref());
        }
        if self.state.view() == View::Panel(Page::Notifications) {
            let hit = self.hit_at(lparam);
            self.notif_hover(client_xy(lparam), hit.as_ref());
        }
        if !self.hover.needs_arming() {
            return;
        }
        self.begin_hover();
    }

    /// The pointer is on the ✕ (or, with `None`, off the island): redraws only
    /// when that changed, so the lit disc follows the pointer with no timer.
    fn track_close_hot(&mut self, lparam: Option<LPARAM>) {
        self.close_ptr = lparam;
        let hot = lparam.is_some_and(|l| self.hit_at(l) == Some(Hit::NoticeClose));
        if hot != self.close_hot {
            self.close_hot = hot;
            let now = Instant::now();
            if self.close_fade.alpha(now) > 0.0 && !self.animating_at(now) {
                self.draw(now);
            }
        }
    }

    /// What the move at `lparam` (client px, `wparam` its button flags) tells
    /// the tooltip: `Some(target)` (a hit with a tip, where the last frame drew
    /// it), `Some(None)` (nothing with a tip: the tip goes) or `None` (leave it
    /// alone: a button is held, the island or its page is moving, or the
    /// pointer is still on the target it pressed; `panel_tips::feed`).
    /// `WM_MOUSEMOVE` gives it to the tooltip once the app is let go.
    fn tip_under(&mut self, wparam: WPARAM, lparam: LPARAM) -> Option<Option<tooltip::Target>> {
        use windows::Win32::System::SystemServices::{MK_LBUTTON, MK_MBUTTON, MK_RBUTTON};
        let buttons = (MK_LBUTTON | MK_RBUTTON | MK_MBUTTON).0 as usize;
        let held = self.mixer_drag.is_some() || wparam.0 & buttons != 0;
        if !held && (self.peek.is_shrunk() || !self.on_island(lparam)) {
            self.tip_on = None;
            self.tip_pressed = None;
            return Some(None);
        }
        let hit = match panel_tips::feed(
            self.hit_at(lparam),
            held,
            self.tip_in_flux(),
            &mut self.tip_pressed,
        ) {
            panel_tips::Feed::LeaveAlone => return None,
            panel_tips::Feed::Nothing => {
                self.tip_on = None;
                return Some(None);
            }
            panel_tips::Feed::Tip(hit) => hit,
        };
        self.tip_on = None;
        // A hit drawn twice has its tip on its first region only: "Buka
        // folder" is the header's icon there and the labelled "N lagi di
        // folder" link under the list.
        let (x, y) = client_xy(lparam);
        if !self.renderer.on_first_region(&hit, x as f32, y as f32) {
            return Some(None);
        }
        Some(self.tip_target(hit))
    }

    /// `hit`'s tooltip target, remembered in `tip_on`; `None` when it says
    /// nothing or the last frame drew no region for it. The tip and its id
    /// are built again only when the hit, what its tip reads or the language
    /// changed since the last move.
    fn tip_target(&mut self, hit: Hit) -> Option<tooltip::Target> {
        let anchor = self.tip_anchor(&hit)?;
        let ctx = self.tip_ctx(&hit);
        let lang = i18n::lang();
        let target = match &self.tip_built {
            Some((h, c, l, t)) if *h == hit && *c == ctx && *l == lang => tooltip::Target {
                anchor,
                ..t.clone()
            },
            _ => {
                let target = tooltip::Target {
                    id: panel_tips::tip_id(&hit),
                    anchor,
                    side: panel_tips::side_for(&hit),
                    tip: panel_tips::tip_for(&hit, &ctx)?,
                };
                self.tip_built = Some((hit.clone(), ctx, lang, target.clone()));
                target
            }
        };
        self.tip_on = Some(IslandTip {
            hit,
            anchor,
            view: self.state.view(),
        });
        Some(target)
    }

    /// The island or the open panel's page is moving: its regions are empty
    /// or about to move, so the hit under the pointer says nothing yet.
    fn tip_in_flux(&self) -> bool {
        self.anim.running
            || self.rise.is_some()
            || self.slide.is_some()
            || self.page_fade.is_some()
            || self.strip_motion.is_some()
    }

    /// A button went down on the island at `lparam`: the target it is on
    /// stays without a tip until the pointer leaves it, through the page turn
    /// or panel change the click may start (`panel_tips::feed`).
    fn tip_press(&mut self, lparam: LPARAM) {
        self.tip_pressed = self.hit_at(lparam).filter(panel_tips::has_tip);
    }

    /// What `hit`'s tip reads of the state: only the piece it needs (the
    /// hovered row's data, not every row's).
    fn tip_ctx(&self, hit: &Hit) -> TipCtx {
        let mut ctx = TipCtx::default();
        match hit {
            Hit::RecMuteCompact(_) | Hit::RecMute(_) => {
                let muted = self.state.rec().map_or([false; 2], |r| r.muted);
                ctx.rec_muted_mic = muted[0];
                ctx.rec_muted_system = muted[1];
            }
            Hit::RecPause => ctx.rec_paused = self.state.rec().is_some_and(|r| r.paused()),
            Hit::RecToggle => {
                ctx.screen_tab = self.state.rec_tab() == RecKind::Screen;
                ctx.recording = self.state.rec().is_some();
            }
            Hit::RecFolder => ctx.screen_tab = self.state.rec_tab() == RecKind::Screen,
            Hit::ScreenDiscard => ctx.screen_discard_armed = self.state.screen_discard_armed(),
            Hit::CalPrev | Hit::CalNext => ctx.cal_view = self.state.cal_view(),
            Hit::PomoStartPause => {
                use crate::pomodoro::PomoStatus;
                match self.state.pomodoro().status() {
                    PomoStatus::Running { .. } => ctx.pomo_running = true,
                    PomoStatus::Paused { .. } => ctx.pomo_paused = true,
                    PomoStatus::Idle | PomoStatus::Ready => {}
                }
            }
            Hit::NoteSticky(id) => {
                ctx.note_on_desktop = self.notes.get(id).is_some_and(|n| n.sticky.is_some());
            }
            Hit::PlayPause => ctx.playing = self.state.media().is_some_and(|m| m.playing),
            Hit::LyricsCard => ctx.lyrics_card_open = self.lyrics_card.open,
            Hit::MixerMute(i) => {
                ctx.mixer = self
                    .mixer
                    .as_ref()
                    .and_then(|rows| rows.get(*i))
                    .map(|row| {
                        // The page names the master level and the system
                        // sounds itself; an app's row carries its name.
                        let name = match row.target {
                            audio::Target::Master => i18n::tr(Key::MixerMaster),
                            audio::Target::System => i18n::tr(Key::MixerSystemSounds),
                            audio::Target::App(_) => row.name.as_str(),
                        };
                        (name.to_string(), row.muted)
                    });
            }
            Hit::ClipPin(id) => {
                ctx.clip_pinned = self.clip.history.get(*id).is_some_and(|c| c.pinned);
            }
            Hit::CliExpand => ctx.cli_tall = self.state.cli_tall(),
            _ => {}
        }
        ctx
    }

    /// Where the last frame drew `hit`, on screen (px): the window's origin
    /// plus the region at the window's scale, as the pointer was mapped.
    fn tip_anchor(&self, hit: &Hit) -> Option<crate::capture::Rect> {
        let win = self.win.as_ref()?;
        let r = self.renderer.region_of(hit)?;
        Some(tooltip::anchor_px(win.screen_origin(), r, win.scale()))
    }

    /// The island's tip goes at once, with no warm window (a wheel, a sink
    /// behind the windows, its target gone).
    fn tip_hide(&mut self) {
        self.tip_on = None;
        tooltip::hide(self.hwnd);
    }

    /// After a state change or a frame: the tip goes once its view changed
    /// (the panel closed, a page turned, fullscreen), the notch shrank to
    /// the eyes, or its target is not where it was (a sliding page or a
    /// moving strip draws no regions, a sub-view swap draws others).
    fn tip_check(&mut self) {
        let Some(on) = &self.tip_on else { return };
        if on.view != self.state.view() || self.eyes || self.tip_anchor(&on.hit) != Some(on.anchor)
        {
            self.tip_hide();
        }
    }

    /// The pointer's first move over the island: Bata wakes, looks happy and
    /// lifts before the dwell opens the panel; hover and leave tracking start.
    fn begin_hover(&mut self) {
        let now = Instant::now();
        self.bata.brain.hover(true, now);
        self.refresh(now);
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_HOVER | TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: HOVER_DWELL_MS,
        };
        if unsafe { TrackMouseEvent(&mut track) }.is_ok() {
            self.hover.armed();
            // The pointer is on the island this visit now: a closable notice
            // gets its ✕.
            if self.sync_close_fade(now) && !self.animating_at(now) {
                self.draw(now);
            }
        }
    }

    fn on_mouse_hover(&mut self, lparam: LPARAM) {
        // The eyes open no panel.
        if self.peek.is_shrunk() {
            self.hover.disarm();
            return;
        }
        // The faint margin round a small island takes a drag, not a hover.
        if !self.on_island(lparam) {
            self.hover.disarm();
            return;
        }
        if let Some(event) = self.hover.on_hover() {
            self.apply_now(event);
        }
    }

    fn on_mouse_leave(&mut self) {
        // A slider drag holds the capture; its release handles the leave.
        if self.mixer_drag.is_some() {
            return;
        }
        // The pointer left the busy toast's spot with the island, whether or
        // not the hover dwell had passed.
        self.set_toast_pointer(false);
        self.track_close_hot(None);
        // A leave, not a reset: a move onto another window's button within
        // the warm window shows its tip at once.
        self.tip_on = None;
        tooltip::hover(self.hwnd, None);
        self.tip_pressed = None;
        self.click.clear();
        self.dock_leave();
        self.arrange_hover(None);
        self.shelf_release();
        self.clip_release();
        self.middle_down = false;
        self.bata.brain.hover(false, Instant::now());
        match self.hover.on_leave() {
            Some(event) => self.apply_now(event),
            // A leave before the dwell sends nothing, but it still ends the
            // visit a drop was made in.
            None if self.state.shelf_drop_here() => self.apply_now(Event::ShelfDropLeft),
            None => self.refresh(Instant::now()),
        }
        // A lit level group of the REC view lets go only after the hover
        // ended (`Hover(false)` already let it go in the reducer): before
        // it, the hover alone would hold the panel open for that one apply,
        // and Hover & stay would latch it. What is left is a leave before
        // the dwell, with no hover to hold anything.
        self.set_rec_mute_hot(None);
    }

    /// The pointer at `lparam` is on the busy toast's own spot: its strip in
    /// the open panel, its small logo (where the full toast would still lie
    /// under the pointer, else there is no peek to give), or, while it peeks,
    /// anywhere on the island.
    fn toast_pointer_at(&self, lparam: LPARAM) -> bool {
        match self.hit_at(lparam) {
            Some(Hit::Strip) => self.state.toast_form() == Some(ToastForm::Strip),
            Some(Hit::ToastSlot) => self.toast_slot_peekable(lparam),
            Some(_) => self.state.toast_is_peeking(),
            None => false,
        }
    }

    /// The full toast would lie under the pointer at `lparam`: a compact or
    /// lyric notch wider than the toast has a band at its right end where a
    /// peek would leave the pointer off it, and the notch would flap.
    fn toast_slot_peekable(&self, lparam: LPARAM) -> bool {
        let (x, y) = client_xy(lparam);
        let (x, y) = self.renderer.to_dip(x as f32, y as f32);
        toast_peek_reaches(self.config.style, self.notch_size(), x, y)
    }

    /// Tells the reducer the pointer reached or left a level group of the REC
    /// view. Only a change is sent, and its redraw lights or clears the pill:
    /// no timer.
    fn set_rec_mute_hot(&mut self, group: Option<crate::recorder::Source>) {
        if self.rec_hot_over != group {
            self.rec_hot_over = group;
            self.apply_now(Event::RecMuteHot(group));
        }
    }

    /// Tells the reducer the pointer reached or left the busy toast's spot.
    fn set_toast_pointer(&mut self, over: bool) {
        if self.toast_over != over {
            self.toast_over = over;
            self.apply_now(Event::ToastPointer(over));
        }
    }

    /// What the button event at `lparam` is over.
    fn hit_at(&self, lparam: LPARAM) -> Option<Hit> {
        // Client coordinates in physical pixels, signed 16-bit each.
        let x = (lparam.0 & 0xFFFF) as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
        self.renderer.hit_test(x, y)
    }

    /// What the pointer at `lparam` (client coordinates) is over.
    fn under_at(&self, lparam: LPARAM) -> Under {
        let (x, y) = client_xy(lparam);
        self.renderer.under(x as f32, y as f32)
    }

    /// The point at `lparam` is a drawn pixel of the island: its shoulders
    /// and anti-aliased edge too, but not the drag margin.
    fn on_island(&self, lparam: LPARAM) -> bool {
        matches!(self.under_at(lparam), Under::Island(_))
    }

    /// `WM_LBUTTONDOWN`: remembers what the press is on.
    fn on_press(&mut self, lparam: LPARAM) {
        self.outside_edge.press_inside();
        let hit = self.hit_at(lparam);
        // The eyes take the press, for the release to restore the notch.
        if self.peek.is_shrunk() {
            self.swipe.cancel();
            self.click.press(hit);
            return;
        }
        self.shelf_press(
            match hit {
                Some(Hit::ShelfCard(i)) => Some(i),
                _ => None,
            },
            lparam,
        );
        self.clip_press(hit.as_ref(), lparam);
        self.mixer_press(hit.as_ref(), lparam);
        if hit.is_some() && self.gestures_active() {
            let (x, y) = client_xy(lparam);
            self.swipe.press(x, y, Instant::now());
        } else {
            self.swipe.cancel();
        }
        self.click.press(hit);
    }

    /// `WM_LBUTTONUP`: a click only when the press was on the island too, on
    /// the same target. A button pressed in another window and let go over
    /// the island does nothing. A swipe is not a click: it skips the song.
    fn on_release(&mut self, lparam: LPARAM) {
        // A click on the eyes brings the notch back and does nothing else:
        // it never opens the panel.
        if self.peek.is_shrunk() {
            let hit = self.hit_at(lparam);
            if self.click.release(hit).is_some() {
                self.restore_peek();
            }
            return;
        }
        self.shelf_release();
        self.clip_release();
        if self.mixer_drag.is_some() {
            self.end_mixer_drag();
            // A drag let go off the island: the moves out there went to the
            // drag, so this is where the pointer leaves.
            if self.hit_at(lparam).is_none() && !self.hover.needs_arming() {
                self.on_mouse_leave();
            }
        }
        let (x, y) = client_xy(lparam);
        let min_px = (gestures::SWIPE_MIN_DIP * self.gesture_scale()).round() as i32;
        let swipe = self.swipe.release(x, y, Instant::now(), min_px);
        if let Some(dir) = swipe.filter(|_| self.gestures_active()) {
            self.click.clear();
            self.apply_now(Event::Click(gesture_hit(dir)));
            return;
        }
        let hit = self.hit_at(lparam);
        if let Some(hit) = self.click.release(hit) {
            // The ✕ may be clicked before the hover dwell: this visit must
            // not turn into a hover that opens the panel under the pointer.
            if hit == Hit::NoticeClose {
                self.hover.close_quiet();
            }
            self.apply_now(Event::Click(hit));
        }
    }

    /// Gestures count while music plays and the island shows it, or its
    /// page is open, and the setting is on.
    fn gestures_active(&self) -> bool {
        self.config.gestures
            && !self.peek.is_shrunk()
            && self.state.media().is_some()
            && matches!(
                self.state.view(),
                View::MediaCompact | View::MediaLyric | View::Panel(Page::Media)
            )
    }

    fn gesture_scale(&self) -> f32 {
        self.win.as_ref().map_or(1.0, IslandWindow::scale)
    }

    /// `WM_RBUTTONDOWN`: the island took the press, so a kept panel's poll
    /// does not count it as a click outside.
    fn on_right_press(&mut self) {
        self.outside_edge.press_inside();
    }

    /// `WM_MBUTTONDOWN`: a middle press on the island, paired with its
    /// release below.
    fn on_middle_press(&mut self, lparam: LPARAM) {
        self.outside_edge.press_inside();
        self.middle_down = self.gestures_active() && self.on_island(lparam);
    }

    /// `WM_MBUTTONUP`: the middle button pressed and let go over the island
    /// plays or pauses. A press that began elsewhere does nothing.
    fn on_middle_release(&mut self, lparam: LPARAM) {
        let pressed = std::mem::take(&mut self.middle_down);
        if middle_click(pressed, self.gestures_active(), self.on_island(lparam)) {
            self.apply_now(Event::Click(Hit::PlayPause));
        }
    }

    /// `WM_MOUSEHWHEEL`: a sideways flick turns one panel page over the
    /// open panel (navigation like the vertical wheel, so not behind
    /// `config.gestures`), or skips the song on the compact music notch
    /// (`gestures::hwheel_route` decides). `lparam` holds the pointer in
    /// screen coordinates. No element of the panel scrolls sideways (the
    /// EDIT controls with `ES_AUTOHSCROLL` are hidden and owner-drawn), so
    /// nothing else claims the flick.
    fn on_hwheel(&mut self, delta: i32, lparam: LPARAM) {
        self.tip_hide();
        let route = gestures::hwheel_route(
            &self.state.view(),
            self.state.cli_menu(),
            self.gestures_active(),
            self.screen_on_island(lparam),
        );
        if route == gestures::HRoute::Nothing {
            return;
        }
        let Some(dir) = self.hwheel.push(delta, Instant::now()) else {
            return;
        };
        match route {
            gestures::HRoute::Page => {
                // Like the vertical wheel: the Music page's cover leaves the
                // next vertical notch to the pages for a moment.
                self.last_paging = Some(Instant::now());
                self.apply_now(Event::PageFlick {
                    next: dir == gestures::Dir::Next,
                });
            }
            gestures::HRoute::Song => self.apply_now(Event::Click(gesture_hit(dir))),
            gestures::HRoute::Nothing => {}
        }
    }

    /// Whether a screen position (a wheel message's `lparam`) is on a drawn
    /// island pixel, not the faint margin round it.
    fn screen_on_island(&self, lparam: LPARAM) -> bool {
        let mut pt = POINT {
            x: (lparam.0 & 0xFFFF) as i16 as i32,
            y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
        };
        unsafe { ScreenToClient(self.hwnd, &mut pt) }.as_bool()
            && matches!(
                self.renderer.under(pt.x as f32, pt.y as f32),
                Under::Island(_)
            )
    }

    /// The wheel scrolls the CLI output while the pointer is over it, and
    /// pages through the panel anywhere else. `lparam` holds the pointer in
    /// screen coordinates.
    fn on_wheel(&mut self, delta: i32, lparam: LPARAM) {
        self.tip_hide();
        // The eyes are no notch: a wheel over them turns nothing.
        if self.peek.is_shrunk() {
            return;
        }
        let mut pt = POINT {
            x: (lparam.0 & 0xFFFF) as i16 as i32,
            y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
        };
        let client = unsafe { ScreenToClient(self.hwnd, &mut pt) }.as_bool();
        // Not on the faint margin round a small island.
        if client
            && !matches!(
                self.renderer.under(pt.x as f32, pt.y as f32),
                Under::Island(_)
            )
        {
            return;
        }
        let over = client && self.renderer.over_cli_output(pt.x as f32, pt.y as f32);
        let over_clips = client && self.renderer.over_clip_list(pt.x as f32, pt.y as f32);
        let over_devices = client && self.renderer.over_devices_list(pt.x as f32, pt.y as f32);
        let over_notifs = client && self.renderer.over_notif_list(pt.x as f32, pt.y as f32);
        let over_art = client && self.renderer.over_media_art(pt.x as f32, pt.y as f32);
        let mole_rows = if client {
            self.renderer.over_mole_log(pt.x as f32, pt.y as f32)
        } else {
            None
        };
        if self.state.cli_menu() && self.state.view() == View::Panel(Page::Cli) {
            // The open folder menu scrolls, a row per notch (more rows
            // than fit: favourites, recent).
            let rows = (-delta / 120).clamp(-20, 20);
            let rows = if rows == 0 { -delta.signum() } else { rows };
            let to = (self.cli_menu_scroll as i64 + i64::from(rows)).max(0) as usize;
            self.cli_menu_scroll = to.min(cli_folders::max_scroll(&self.cli_menu_all));
            self.rebuild_cli_menu();
            self.redraw_cli();
        } else if let Some(row) = self.mixer_row_under(client, &pt) {
            self.mixer_wheel(delta, row);
        } else if over_clips
            && self.state.view() == View::Panel(Page::Clipboard)
            && self.clip_wheel(delta)
        {
            // The cards scrolled (or an end spent the notch).
        } else if over_devices
            && self.state.view() == View::Panel(Page::Arrange)
            && self.arrange_wheel(delta)
        {
            // The Susun list scrolled; with everything fitting, the notch
            // turns the page below.
        } else if over_devices
            && self.state.view() == View::Panel(Page::Devices)
            && self.devices_wheel(delta)
        {
            // The device cards scrolled; with everything fitting, the notch
            // turns the page below.
        } else if over_notifs
            && self.state.view() == View::Panel(Page::Notifications)
            && self.notif_wheel(delta)
        {
            // The rows scrolled (or an end spent the notch); with everything
            // fitting, the notch turns the page below.
        } else if over && self.state.view() == View::Panel(Page::Cli) {
            // A notch away from the user scrolls up to older lines.
            let lines = (delta * WHEEL_LINES / 120).clamp(-60, 60);
            let lines = if lines == 0 { delta.signum() } else { lines };
            self.cli_scroll(lines);
        } else if let Some(rows) = mole_rows
            && self.state.view() == View::Panel(Page::System)
            && self.state.cleaner().is_some()
            && self.mole.log.len() > rows
        {
            // The cleaner keeps the wheel; over a log longer than its box
            // the notch scrolls it (away from the user: older lines).
            let lines = (delta * WHEEL_LINES / 120).clamp(-60, 60);
            let lines = if lines == 0 { delta.signum() } else { lines };
            let max = self.mole.log.len().saturating_sub(rows);
            self.apply_now(Event::MoleLogScroll { lines, max });
        } else if volume_wheel_applies(
            self.config.scroll_volume,
            self.state.media().is_some_and(|m| m.playing),
            &self.state.view(),
            over_art,
            paging_recent(self.last_paging, Instant::now()),
        ) {
            self.volume_wheel(delta);
        } else {
            if matches!(self.state.view(), View::Panel(_)) {
                self.last_paging = Some(Instant::now());
            }
            self.apply_now(Event::Wheel(delta));
        }
    }

    /// The Mixer row the wheel is over (its speaker or slider), while the
    /// page shows.
    fn mixer_row_under(&self, client: bool, pt: &POINT) -> Option<usize> {
        if !client || self.state.view() != View::Panel(Page::Mixer) {
            return None;
        }
        match self.renderer.hit_test(pt.x as f32, pt.y as f32)? {
            Hit::MixerSlider(i) | Hit::MixerMute(i) => Some(i),
            _ => None,
        }
    }

    /// Writes the island's own settings into config.json as it is now, so
    /// fields edited by hand meanwhile survive (and are picked up). An invalid
    /// file is backed up first (`config::save_merged_at`); the first time this
    /// run, a notice says so. The setting holds for this run either way.
    fn save_config(&mut self) {
        let out = config::save_merged(&self.config);
        self.adopt_config(out.config);
        if out.repair != Repair::None && !self.config_warned {
            self.config_warned = true;
            self.apply_now(Event::Notice(invalid_config_notice(Some(&out.repair))));
        }
    }

    /// `save_config`, also writing the fields `edit` sets: ones the owner may
    /// edit by hand (Google client, battery threshold) but just set in the
    /// settings window. Other hand edits in the file stay.
    fn save_config_with(&mut self, edit: impl FnOnce(&mut Config)) {
        let out = config::save_merged_with(&self.config, edit);
        self.adopt_config(out.config);
        if out.repair != Repair::None && !self.config_warned {
            self.config_warned = true;
            self.apply_now(Event::Notice(invalid_config_notice(Some(&out.repair))));
        }
    }

    /// Picks up hand edits to config.json. False when the file exists but is
    /// not a valid config; it then changes nothing (only saves back it up).
    fn reload_config(&mut self) -> bool {
        match config::read_at(&config::path()) {
            OnDisk::Valid(disk) => {
                let merged = config::merge_owned(disk, &self.config);
                self.adopt_config(merged);
                true
            }
            OnDisk::Missing => true,
            OnDisk::Invalid => false,
        }
    }

    /// The sync thread gets a changed Google client before any later request.
    fn adopt_config(&mut self, cfg: Config) {
        let google = |c: &Config| (c.google_client_id.clone(), c.google_client_secret.clone());
        let changed = google(&cfg) != google(&self.config);
        let toast_style_was = self.config.notifications.toast_style;
        let lang_was = self.config.language;
        self.config = cfg;
        // Palette, surface, accent, fill, notch size, style and font may all
        // have changed: the next frame is drawn in full.
        self.renderer.invalidate();
        i18n::set_lang(self.config.language);
        if self.config.language != lang_was {
            crate::ui::tooltip::hide_all();
        }
        crate::ui::tooltip::set_hotkeys(self.config.hotkeys);
        crate::ui::tooltip::set_font(self.config.font);
        // A hand edit of "Notifikasi saat notch dipakai" applies live.
        let toast_style = self.config.notifications.toast_style;
        if toast_style != toast_style_was {
            self.apply_now(Event::ToastStyle(toast_style));
        }
        // New limits (the settings window or a hand edit) apply at once.
        self.system
            .monitor
            .set_limits(self.config.disk_warn_gb, self.config.ram_warn_percent);
        self.clip_follow_config();
        // The cap, the file and the muted apps of the Notifikasi history.
        self.notif_follow_config();
        // The hotkeys follow a changed value (once started; not while
        // Settings records).
        self.hotkeys_follow_config();
        // A hand edit of the recorder's sources reaches the Rekam page (not
        // while a recording runs: it keeps what it started with).
        if !self.rec.active() {
            self.rec_sources = self.config.recording.sources;
        }
        if changed && let Some(sync) = &self.sync {
            sync.send(SyncRequest::Config(self.config.clone()));
            // The next calendar view asks again, now with the new client.
            self.calendar_asked = None;
        }
        self.apply_theme();
        self.apply_ui_font();
        // The desktop widgets: their settings, notes, colours and font.
        self.widgets_follow_config();
        // An open transcript window: its language and font.
        self.transcript_refresh_all();
        // Stay-behind, the cat and its motion decide the pointer poll.
        self.follow_pointer_settings(Instant::now());
        // A hand edit of the language (or the font) reaches an open lyrics
        // card here; nothing is sent when nothing it shows changed.
        self.sync_lyrics_card(Instant::now());
        // The same for an open caption card.
        self.captions_card_sync();
    }

    /// Puts a changed UI font (from Settings) into effect now: the
    /// renderer rebuilds its formats, the lyric line on screen is measured
    /// again (the notch is as wide as its line, `lyric_text_width`) and
    /// everything redraws. Compared with the font in effect, so a caller may
    /// set the field first.
    fn apply_ui_font(&mut self) {
        let font = self.config.font;
        if self.renderer.ui_font() == font {
            return;
        }
        if let Err(error) = self.renderer.set_ui_font(font) {
            eprintln!("vale: cannot set the UI font: {error}");
            return;
        }
        let now = Instant::now();
        if self.state.view() == View::MediaLyric {
            self.fit_lyric_line(now);
        }
        self.refresh(now);
        self.draw(now);
    }

    /// Puts a changed accent, surface or window mode into effect: the island
    /// redraws and an open editor restyles. Compared with the theme in effect
    /// rather than the old config, so a caller may set the fields first.
    fn apply_theme(&mut self) {
        if theme::set_current(Theme::of(&self.config)) {
            crate::ui::tooltip::hide_all();
            editor::restyle_open_editor();
            crate::shot_edit::window::restyle_open();
            settings::restyle();
            crate::transcript_win::restyle();
            self.draw(Instant::now());
        }
    }

    fn set_style(&mut self, style: Style) {
        if self.config.style == style {
            return;
        }
        self.config.style = style;
        self.save_config();
        let now = Instant::now();
        if self.state.view() == View::Hidden {
            // Nothing to morph; rest on the new shape so the next appearance grows from it.
            let rest = shape_sized(style, self.notch_size(), ShapeKind::Idle);
            self.anim = Anim::settled(rest, 0.0);
            self.draw(now);
        }
        // Visible: morphs from the old style's shape to the new one.
        self.refresh(now);
    }

    fn set_autostart(&mut self, enabled: bool) {
        if tray::set_autostart(enabled).is_ok() {
            self.config.start_with_windows = enabled;
            self.save_config();
        }
    }

    /// What the settings window shows.
    fn settings_model(&self) -> settings::Model {
        let status = self.state.pomodoro().status();
        settings::Model {
            sticky_count: self.notes.sticky_count(),
            pomodoro: self.state.pomodoro().settings(),
            pomodoro_locked: !matches!(status, PomoStatus::Idle | PomoStatus::Ready),
            calendar: self.calendar.status,
            hotkey_failed: self.hotkeys.failed_flags(),
            update_row: self.update.row(),
            transfer: self.transfer.view.clone(),
            transcript: self.transcript_card_view(),
            monitors: self
                .displays
                .iter()
                .map(MonitorRow::from_display)
                .collect(),
            ..settings::Model::from_config(&self.config)
        }
    }

    /// The screen the notch shows on, from Settings or the tray. Saves the
    /// choice and says whether it changed; the caller then moves the island
    /// (`reposition`, or `DEFERRED_REPOSITION` while the app is borrowed).
    fn set_notch_monitor(&mut self, choice: NotchMonitor) -> bool {
        if self.config.notch_monitor == choice {
            return false;
        }
        self.config.notch_monitor = choice;
        self.save_config();
        true
    }

    /// "Tandai": numbers every screen for two seconds. The list is read again
    /// first, so a screen plugged in a moment ago has its plate.
    fn identify_monitors(&mut self) {
        self.displays = arrange_win::displays();
        let fade = self.motion().moves() && self.animations;
        identify_overlay::show(&self.displays, fade);
    }

    /// Brings an open settings window up to date; it redraws only on a change.
    fn sync_settings(&self) {
        if settings::is_open() {
            settings::update(self.settings_model());
        }
    }

    /// A change made in the settings window: applied now and saved.
    fn on_settings_action(&mut self, action: SettingsAction) {
        let now = Instant::now();
        match action {
            SettingsAction::Style(style) => self.set_style(style),
            SettingsAction::Accent(accent) => {
                self.config.accent = accent;
                self.save_config();
            }
            SettingsAction::Surface(surface) => {
                self.config.surface = surface;
                self.save_config();
            }
            // `adopt_config` (from the save) switches the typeface live.
            SettingsAction::Font(font) => {
                self.config.font = font;
                self.save_config();
            }
            // A live step follows the pointer and saves nothing; the release
            // (`commit`) saves once.
            SettingsAction::NotchSize { w, h, commit } => {
                self.set_notch_size(w, h, !commit);
                if commit {
                    self.save_config();
                }
            }
            SettingsAction::NotchOpacity { pct, commit } => {
                self.set_notch_opacity(pct);
                if commit {
                    self.save_config();
                }
            }
            SettingsAction::NotchFill(fill) => {
                self.config.notch_fill = fill;
                self.on_fill_changed();
                self.save_config();
            }
            SettingsAction::NotchReset => {
                self.set_notch_size(NOTCH_W_DEFAULT, NOTCH_H_DEFAULT, false);
                self.save_config();
            }
            // Runs inside `with_app`, so the island moves once the handler
            // returns (`run_deferred`, at the end of the window procedure),
            // never from here.
            SettingsAction::NotchMonitor(choice) => {
                if self.set_notch_monitor(choice) {
                    DEFERRED_REPOSITION.set(true);
                }
            }
            SettingsAction::NotchIdentify => self.identify_monitors(),
            SettingsAction::WindowMode(mode) => {
                self.config.window_theme = mode;
                self.save_config();
            }
            SettingsAction::Lang(lang) => {
                self.config.language = lang;
                // `adopt_config` switches the strings; everything showing text relabels.
                self.save_config();
                editor::relabel_open_editor();
                // The lyrics card draws its own messages in this language.
                self.sync_lyrics_card(now);
                self.draw(now);
            }
            SettingsAction::Autostart(on) => self.set_autostart(on),
            SettingsAction::IdleClock(on) => {
                self.config.idle_clock = on;
                self.save_config();
                // Redraws the idle island and starts or stops its timer.
                self.refresh(now);
            }
            SettingsAction::Lyrics(on) => {
                self.config.lyrics = on;
                self.save_config();
                // Looks the track up and switches the view now.
                self.apply_now(Event::LyricsSetting(on));
            }
            SettingsAction::Robot(on) => {
                self.config.robot = on;
                self.save_config();
                // The compact shapes widen or narrow; its timers start or stop.
                self.refresh(now);
            }
            SettingsAction::WidgetsShow(on) => self.widgets_show(on),
            SettingsAction::WidgetsLayer(layer) => self.widgets_layer(layer),
            SettingsAction::WidgetsFullscreen(rule) => self.widgets_fullscreen(rule),
            SettingsAction::WidgetsLock(on) => self.widgets_lock(on),
            SettingsAction::WidgetsPeek(on) => self.widgets_peek(on),
            SettingsAction::WidgetsTilt(on) => self.widgets_tilt(on),
            SettingsAction::WidgetsClipFill(on) => self.widgets_clip_fill(on),
            SettingsAction::WidgetAdd(template) => {
                self.new_sticky(None, template);
            }
            SettingsAction::WidgetHighlight => self.widgets_highlight(),
            SettingsAction::StayBehind(on) => {
                self.config.stay_behind = on;
                self.save_config();
                // `sync_behind` starts or stops the poll and sets the order.
                self.refresh(now);
            }
            SettingsAction::RobotMotion(motion) => {
                self.config.robot_animation = motion;
                self.save_config();
                self.refresh(now);
            }
            SettingsAction::MascotPlay(play) => {
                self.config.mascot_play = play;
                self.save_config();
                // The armed wait was rolled for the old frequency: drop it so
                // the next sync rolls a fresh one. A deferred play owns the
                // timer while `pending` is set, so leave it then.
                if self.pending.is_none() && self.mascot_timer {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TIMER_MASCOT_PLAY);
                    }
                    self.mascot_timer = false;
                }
                self.refresh(now);
            }
            SettingsAction::RobotSleep(minutes) => {
                self.config.robot_sleep_minutes = robot::clamp_sleep(minutes);
                self.save_config();
                self.refresh(now);
            }
            SettingsAction::Mascot(mascot) => {
                // Only the choice: each mascot keeps its own coat.
                self.config.mascot = mascot;
                self.save_config();
                // Another mascot, and (on Auto) the island's surface.
                self.refresh(now);
            }
            SettingsAction::CatColor(color) => {
                self.config.cat_color = color;
                self.save_config();
                // The cat and (on Auto) the island's surface change now.
                self.draw(now);
            }
            SettingsAction::FalkColor(color) => {
                self.config.falk_color = color;
                self.save_config();
                self.draw(now);
            }
            SettingsAction::OctopusColor(color) => {
                self.config.octopus_color = color;
                self.save_config();
                self.draw(now);
            }
            SettingsAction::CatAccessory(accessory) => {
                self.config.cat_accessory = accessory;
                self.save_config();
                self.draw(now);
            }
            SettingsAction::CatFollow(on) => {
                self.config.cat_follow_pointer = on;
                self.save_config();
                // `follow_pointer_settings` (from the save) starts or stops
                // the pointer poll and puts the look back at the centre.
                self.refresh(now);
            }
            SettingsAction::CatDance(on) => {
                self.config.cat_dance = on;
                self.save_config();
                // `sync_eq` (from `refresh`) starts or stops `TIMER_EQ` now,
                // and the cat is drawn at rest or dancing.
                self.refresh(now);
            }
            SettingsAction::NotchPeek(on) => {
                self.config.notch_peek = on;
                self.save_config();
                // A shrunk notch comes back at once; the poll follows.
                self.refresh(now);
            }
            SettingsAction::EqFollowAudio(on) => {
                self.config.eq_follow_audio = on;
                self.save_config();
                // `sync_eq` (from `refresh`) starts or stops the capture now.
                self.refresh(now);
            }
            SettingsAction::Greeting(on) => {
                self.config.morning_greeting = on;
                self.save_config();
            }
            SettingsAction::Hotkey(id, hk) => self.set_hotkey(id, hk),
            SettingsAction::HotkeysResetAll => self.reset_all_hotkeys(),
            SettingsAction::HotkeyRecording(on) => {
                if on {
                    self.pause_hotkeys();
                } else {
                    self.resume_hotkeys();
                    self.sync_settings();
                }
            }
            SettingsAction::DeviceNotices(on) => {
                self.config.device_notices = on;
                self.save_config();
            }
            SettingsAction::RecSources(v) => self.rec_set_sources(v),
            SettingsAction::RecDevice(source, id) => self.rec_set_device(source, id),
            SettingsAction::RecFolder(dir) => self.rec_set_folder(dir),
            SettingsAction::ConfigExport => self.config_export(),
            SettingsAction::ConfigExportTo(path) => self.config_export_to(path),
            SettingsAction::ConfigImport => self.config_import(),
            SettingsAction::ConfigImportFrom(path) => self.config_import_from(path),
            SettingsAction::ConfigImportConfirm => self.config_import_confirm(),
            SettingsAction::ConfigImportCancel => self.config_import_cancel(),
            SettingsAction::VideoFps(fps) => self.screen_set_fps(fps),
            SettingsAction::VideoQuality(q) => self.screen_set_quality(q),
            SettingsAction::CursorHighlight(on) => {
                self.config.recording.cursor_highlight = on;
                self.save_config();
            }
            SettingsAction::ClickRipples(on) => {
                self.config.recording.click_ripples = on;
                self.save_config();
            }
            SettingsAction::TranscribeFile(path) => self.transcribe_file(path),
            SettingsAction::RecEchoCancel(on) => {
                self.config.recording.echo_cancel = on;
                self.save_config();
            }
            SettingsAction::TranscriptEnabled(on) => self.transcribe_set_enabled(on),
            SettingsAction::TranscriptModel(key) => self.transcribe_pick(key),
            SettingsAction::TranscriptDownload(key) => self.transcribe_download_key(key),
            SettingsAction::TranscriptCancelDownload => self.transcribe_cancel_download(),
            SettingsAction::TranscriptDelete(key) => {
                if let Some(model) = crate::transcribe::catalog::model(key) {
                    self.transcribe_delete(model);
                }
            }
            SettingsAction::TranscriptLang(language) => {
                self.config.transcript.language = language;
                self.save_config();
            }
            SettingsAction::TranscriptAuto(on) => {
                self.config.transcript.auto = on;
                self.save_config();
            }
            SettingsAction::TranscriptFolder => self.transcribe_open_folder(),
            // A changed "Mic juga" restarts a running worker (its channels
            // and labels change); the others are read by `captions_sync`.
            SettingsAction::CaptionMic(on) => {
                self.config.captions.mic = on;
                self.save_config();
                self.captions_restart();
            }
            SettingsAction::CaptionNotch(on) => {
                self.config.captions.notch = on;
                self.save_config();
                self.captions_sync();
                // On or off, the line follows at once.
                if self.captions_notch_sync() {
                    self.refresh(Instant::now());
                }
            }
            SettingsAction::CaptionBattery(on) => {
                self.config.captions.on_battery = on;
                self.save_config();
                self.captions_sync();
            }
            // The thumb moves live; only the release is saved.
            SettingsAction::ArrangeGap { gap, commit } => {
                self.config.arrange.gap = gap;
                if commit {
                    self.save_config();
                }
            }
            SettingsAction::ArrangeTemplates(list) => {
                self.config.arrange.templates = list;
                self.save_config();
                self.sync_settings();
                self.arrange_config_changed();
            }
            SettingsAction::ArrangeSaved(list) => {
                self.config.arrange.saved = list;
                self.save_config();
                self.sync_settings();
                self.arrange_config_changed();
            }
            SettingsAction::OpenMode(mode) => {
                self.config.open_mode = mode;
                self.save_config();
                self.apply_now(Event::OpenMode(mode));
            }
            SettingsAction::NotifToastStyle(v) => {
                self.config.notifications.toast_style = v;
                self.save_config();
                self.apply_now(Event::ToastStyle(v));
            }
            SettingsAction::CliStart(start) => {
                // Read the next time the CLI page opens.
                self.config.cli_start = start;
                self.save_config();
            }
            SettingsAction::Shell(shell) => {
                // The next command runs in it; one running goes on as it is.
                self.config.cli_shell = shell;
                self.save_config();
                self.draw(now);
            }
            SettingsAction::BatteryThreshold(threshold) => {
                self.save_config_with(|c| c.battery_threshold = threshold);
                self.battery.set_threshold(self.config.battery_threshold);
            }
            // `adopt_config` hands the new limits to the warnings; the open
            // System page redraws its meters (ochre or not).
            SettingsAction::DiskWarn(gb) => {
                self.save_config_with(|c| c.disk_warn_gb = gb);
                self.redraw_system_page();
            }
            SettingsAction::RamWarn(percent) => {
                self.save_config_with(|c| c.ram_warn_percent = percent);
                self.redraw_system_page();
            }
            SettingsAction::MoleProtect(on) => {
                // Read before Mole's next preview.
                self.config.mole_protect_defaults = on;
                self.save_config();
            }
            SettingsAction::PanelPages(pages) => {
                self.config.panel_pages = pages.to_saved();
                self.save_config();
                // The wheel, the dots and the page shown follow at once.
                self.apply_now(Event::PanelPages(pages));
            }
            SettingsAction::ShortcutTiles(tiles) => {
                self.config.shortcut_tiles = tiles.to_saved();
                self.save_config();
                self.apply_now(Event::ShortcutTiles(tiles));
            }
            SettingsAction::WindowPlace(p) => {
                self.save_config_with(|c| c.settings_window = Some(p))
            }
            SettingsAction::Pomo(field, dir) => {
                // Through the reducer like a click: state, then config.
                self.apply_now(Event::Click(Hit::PomoAdjust(field, dir)));
            }
            SettingsAction::SaveGoogle { id, secret } => {
                self.save_config_with(|c| {
                    c.google_client_id = id;
                    c.google_client_secret = secret;
                });
            }
            SettingsAction::ConnectGoogle => {
                if let Some(event) = self.connect_google() {
                    self.apply_now(event);
                }
            }
            SettingsAction::DisconnectGoogle => self.disconnect_google(),
            SettingsAction::ClipEnabled(on) => self.on_clip_setting(ClipSetting::Enabled(on)),
            SettingsAction::ClipItems(n) => self.on_clip_setting(ClipSetting::Items(n)),
            SettingsAction::ClipPersist(on) => self.on_clip_setting(ClipSetting::Persist(on)),
            SettingsAction::ClipClearOnLock(on) => {
                self.on_clip_setting(ClipSetting::ClearOnLock(on))
            }
            SettingsAction::ClipFeedbackAll(on) => {
                self.on_clip_setting(ClipSetting::FeedbackAll(on))
            }
            SettingsAction::ClipNotice(on) => self.on_clip_setting(ClipSetting::CopyNotice(on)),
            SettingsAction::ClipClear => self.on_clip_setting(ClipSetting::Clear),
            SettingsAction::NotifRecord(on) => self.on_notif_setting(NotifSetting::Record(on)),
            SettingsAction::NotifItems(n) => self.on_notif_setting(NotifSetting::Items(n)),
            SettingsAction::NotifPersist(on) => self.on_notif_setting(NotifSetting::Persist(on)),
            SettingsAction::NotifMutedAdd(name) => {
                self.on_notif_setting(NotifSetting::MutedAdd(name))
            }
            SettingsAction::NotifMutedRemove(name) => {
                self.on_notif_setting(NotifSetting::MutedRemove(name))
            }
            SettingsAction::NotifClear => self.on_notif_setting(NotifSetting::Clear),
            SettingsAction::ShotTool(v) => self.on_shot_setting(ShotSetting::Tool(v)),
            SettingsAction::ShotPreview(on) => self.on_shot_setting(ShotSetting::Preview(on)),
            SettingsAction::ShotOnIgnore(v) => self.on_shot_setting(ShotSetting::OnIgnore(v)),
            SettingsAction::ShotSecs(n) => self.on_shot_setting(ShotSetting::Secs(n)),
            SettingsAction::ShotAutoCopy(on) => self.on_shot_setting(ShotSetting::AutoCopy(on)),
            SettingsAction::ShotAfterCapture(v) => {
                self.on_shot_setting(ShotSetting::AfterCapture(v));
            }
            SettingsAction::CheckUpdates => self.update_manual_check(),
            SettingsAction::InstallUpdate => self.update_install(),
            SettingsAction::UpdateAuto(on) => {
                self.config.update_check = on;
                self.save_config();
            }
            // Handled outside the app borrow (`deliver_settings_action`).
            SettingsAction::DndHelp => {}
            // A window closed in the middle of a recording: the hotkeys come back.
            SettingsAction::Closed => {
                self.resume_hotkeys();
                // A checked file, a result or a refusal does not outlive the
                // window; the cancel path also drops the file.
                self.config_import_cancel();
            }
        }
        self.sync_settings();
    }

    fn redraw_system_page(&mut self) {
        if system_sample_wanted(&self.state.view(), self.state.cleaner().is_some())
            && !self.animating()
        {
            self.draw(Instant::now());
        }
    }

    /// Forgets the Google login; the calendar asks to connect again.
    fn disconnect_google(&mut self) {
        if let Some(sync) = &self.sync {
            sync.send(SyncRequest::Disconnect);
        }
        if self.calendar.status != CalStatus::NotConfigured {
            self.calendar.status = CalStatus::NeedsLogin;
        }
        self.calendar_asked = None;
        self.refresh(Instant::now());
    }

    fn finish_onboarding(&mut self) {
        self.config.onboarding_done = true;
        self.save_config();
    }

    /// The slot's share of the drawn compact view's width at `now` (0 with
    /// no slot, or on any other view).
    fn slot_drawn(&self, now: Instant) -> f32 {
        match (&self.drawn, self.slot_keep.is_some()) {
            (
                View::MediaCompact
                | View::MediaLyric
                | View::PomodoroCompact
                | View::RecordCompact
                | View::ScreenCountdown(_),
                true,
            ) => {
                self.slot_share.at(&self.anim, now)
            }
            _ => 0.0,
        }
    }

    /// Draws the frame now.
    fn draw(&mut self, now: Instant) {
        self.draw_frame(now, false);
    }

    /// The `TIMER_EQ` tick: draws only when the frame would differ from the
    /// last one drawn (`dirty::eq_draw_needed`). Nothing the app changed
    /// since (`draw_gen`), no new bar heights and nothing time-based (the
    /// sine bars, a moving progress line, the dancing cat's floating note)
    /// means the screen already shows this frame.
    fn draw_eq_tick(&mut self, now: Instant) {
        self.draw_frame(now, true);
    }

    fn draw_frame(&mut self, now: Instant, eq_tick: bool) {
        self.dock_sync();
        // Clicks on the Devices page mean the cards of the frame drawn here.
        self.devices_sync_drawn();
        let Some(win) = &self.win else { return };
        let (shape, opacity) = self.anim.frame(now);
        let (shape, opacity) = rise_frame(shape, opacity, self.rise.as_ref(), now);
        let morphing = self.anim.running;
        // The closing panel draws what it showed, not what its close reset.
        let leaving = self.leaving.as_ref().filter(|_| morphing);
        let transit = Transit {
            reveal: self.reveal.map(|r| r.frame(now)),
            // Its last frame (t = 1) is drawn settled, with click regions.
            page_fade: self
                .page_fade
                .and_then(|f| page_fade_frame(f.progress(now))),
            content_alpha: if morphing {
                self.anim
                    .fade_from
                    .map_or(1.0, |from| content_alpha_from(from, self.anim.fade_t(now)))
            } else {
                1.0
            },
            leaving: leaving.map(|shown| shown.page),
            fade_floor_h: self.fade_floor_h,
        };
        let fill_alpha = self.fill_at(now);
        let state = &self.state;
        let cwd = self.cli_cwd.to_string_lossy();
        let input = self.cli_input.as_ref();
        let typed = input.map(CliInput::text).unwrap_or_default();
        let selection = input.map_or((0, 0), CliInput::selection);
        let focused = state.cli_focused();
        // As `cli_suggestions`, borrowing only the history.
        let suggestions = if self.cli_browsing || !focused {
            Vec::new()
        } else {
            cli_panel::suggestions(self.cli_history.items(), &typed, MAX_SUGGESTIONS)
        };
        let robot = self.robot_frame(&self.drawn, now);
        let eased = self.system.ease.figures(now);
        let system = SystemView {
            sample: self.system.sample.as_ref(),
            eased: eased.as_ref(),
            history: self.system.history.points(),
            disk_warn_gb: self.config.disk_warn_gb,
            ram_warn_percent: self.config.ram_warn_percent,
        };
        let (cleaner_ui, mole_phase) = match leaving {
            Some(shown) => (shown.cleaner, shown.mole),
            None => (state.cleaner().copied(), state.mole_phase()),
        };
        let cleaner = cleaner_ui.map(|c| CleanerView {
            mode: c.mode,
            checked: c.checked,
            armed: c.armed(),
            scan: self.clean.scan.as_ref(),
            scanning: self.clean.scanning,
            busy: state.clean_busy(),
            mole: MoleView {
                phase: mole_phase,
                opts: state.mole_opts(),
                show_log: c.mole_log,
                version: self
                    .mole
                    .install
                    .as_ref()
                    .and_then(|i| i.version.as_deref()),
                tested: self
                    .mole
                    .install
                    .as_ref()
                    .is_some_and(mole::Install::tested),
                summary: self.mole.preview.as_ref().map(mole::Preview::summary),
                log: &self.mole.log,
                report: self.mole.report.as_ref(),
                scroll: state.mole_log_scroll(),
            },
        });
        let search = self.clip_input.as_ref();
        let query = search.map(CliInput::text).unwrap_or_default();
        let search_sel = search.map_or((0, 0), CliInput::selection);
        let clip_focused = state.clip_focused();
        let search_field = self.search_input.as_ref();
        let search_text = search_field.map(CliInput::text).unwrap_or_default();
        let search_selection = search_field.map_or((0, 0), CliInput::selection);
        let search_open = state.search_open();
        let clip = clip::clip_view(
            &self.clip,
            self.config.clipboard.enabled,
            &query,
            search_sel,
            clip_focused,
        );
        let notif =
            notif_page::notif_view(&self.notif, self.config.notifications, state.notif_armed());
        // A busy toast's logo beside a compact view: the slot's share of the
        // animated shape's width.
        let toast_slot_w = self.slot_drawn(now);
        let logos = &self.notif.logos;
        // The strip: a toast's while its form is the strip, else the sticky
        // one, and the toast that is fading out of it for the motion's time.
        let strip = match leaving {
            Some(shown) => shown.strip.as_ref(),
            None => state
                .waiting_strip()
                .or_else(|| self.strip_motion.as_ref().and_then(|m| m.leaving.as_ref())),
        };
        let phone_view = App::phone_view(state, &self.phone, self.config.phone_net, now);
        // Anything that moves outside the shape, or the shape itself: the
        // renderer redraws the whole surface (`ui::dirty`).
        let full_redraw = self.anim.running
            || self.rise.is_some()
            || self.slide.is_some()
            || self.reveal.is_some()
            || self.page_fade.is_some()
            || leaving.is_some()
            || transit.content_alpha < 1.0
            || self.strip_motion.is_some()
            || self.eyes_morph;
        let rec_levels = self.rec_levels();
        let frame = Frame {
            view: &self.drawn,
            media: state.media(),
            pomodoro: state.pomodoro(),
            caption: match &self.drawn {
                View::CaptionCompact => state.caption_line(),
                View::RecordCompact => state.rec_caption(),
                _ => None,
            },
            rec: state.rec().map(|r| RecFrame {
                elapsed: r.elapsed(now),
                paused: r.paused(),
                sources: r.sources,
                mic: rec_levels.0,
                system: rec_levels.1,
                muted: r.muted,
                hot: state.rec_mute_hot(),
                screen: r.kind == RecKind::Screen,
                zoom: r.zoom,
            }),
            now,
            today: state.today(),
            page: leaving.map_or(state.page(), |shown| shown.page),
            strip,
            cal_view: state.cal_view(),
            cal_anchor: state.cal_anchor(),
            cal_selected: state.cal_selected(),
            calendar: &self.calendar,
            mixer: self.mixer.as_deref(),
            shelf: &self.shelf.cards,
            shelf_drag: self.shelf.dragging,
            shelf_motion: &self.shelf.motion,
            phone: phone_view.as_ref(),
            phone_on: self.phone.running(),
            arrange: App::arrange_view(&self.arrange, &self.config, self.search_input.as_ref()),
            record: RecordView {
                sources: self.rec_sources,
                recent: state.rec_list(),
                devices: self.rec.devices(),
                tab: state.rec_tab(),
                discard_armed: state.screen_discard_armed(),
                captions_on: self.captions_on(),
                total: match state.rec_tab() {
                    RecKind::Audio => self.rec.total,
                    RecKind::Screen => self.screen.total,
                },
                missing: self.rec.missing(),
                quality: i18n::rec_quality_label(
                    i18n::lang(),
                    self.config.recording.video_quality,
                    self.config.recording.video_fps,
                ),
                mute_hotkey: self.config.hotkeys.record_mute.map(|h| h.to_string()),
                start_hotkey: match state.rec_tab() {
                    RecKind::Audio => self.config.hotkeys.record,
                    RecKind::Screen => self.config.hotkeys.record_screen,
                }
                .map(|h| h.to_string()),
                today: state.today(),
            },
            devices: DevicesView {
                list: &self.devices.list,
                ejecting: &self.devices.ejecting,
                vetoes: &self.devices.vetoes,
                expanded: self.devices.expanded.as_deref(),
                scroll: self.devices.scroll,
            },
            notes: self.notes.list(),
            reminders: self.reminders.upcoming(),
            reminders_page: leaving.map_or(state.reminders_page(), |shown| shown.reminders_page),
            notes_page: leaving.map_or(state.notes_page(), |shown| shown.notes_page),
            day_page: leaving.map_or(state.day_page(), |shown| shown.day_page),
            page_slide: self.slide.map_or(0.0, |s| s.offset(now)),
            page_trail: self
                .slide
                .map_or([None; 2], |s| s.strip.visible(s.offset(now))),
            page_from: self.page_fade.map(|f| f.from_page),
            pages: state.pages().visible(),
            dock: self.dock.frame(),
            tiles: state.tiles(),
            palette: theme::island_palette(&self.config),
            cat: CatLook::of(&self.config),
            fill_alpha,
            legibility: notch_fill::legibility(fill_alpha),
            clock: (self.config.idle_clock && self.drawn == View::Idle)
                .then(|| Local::now().format("%H:%M").to_string()),
            // The idle island's growth on the left (the typing cat's
            // laptop), so the robot and the clock stay still as it morphs.
            // Measured against the plain idle shape, so a symmetric morph
            // back to idle (media, a notice or the panel ending) keeps the
            // group centred.
            idle_lead: if self.drawn == View::Idle && self.config.robot {
                let plain = shape_with_robot_sized(
                    self.config.style,
                    self.notch_size(),
                    ShapeKind::Idle,
                    true,
                );
                typing_lead(shape, plain)
            } else {
                0.0
            },
            lyric: self.lyrics.shown(now),
            lyric_scroll: if self.drawn == View::MediaLyric {
                self.lyric_fit.scroll(now)
            } else {
                0.0
            },
            lyric_prev_scroll: self.lyric_fit.prev_scroll,
            lyrics_on: state.lyrics_on(),
            lyrics_card_open: self.lyrics_card.open,
            volume: self.volume_hud,
            cli: CliView {
                output: &self.cli.output,
                echoes: &self.cli_echoes,
                scroll: state.cli_scroll(),
                run: self.cli_clock.state(),
                shell: self.config.cli_shell,
                cwd: &cwd,
                input: &typed,
                selection,
                focused,
                suggestions,
                selected: self.cli_pick,
                tall: leaving.map_or(state.cli_tall(), |shown| shown.cli_tall),
                from_explorer: self.cli_from_explorer,
                menu: leaving
                    .map_or(state.cli_menu(), |shown| shown.cli_menu)
                    .then_some(&self.cli_menu_view),
            },
            clip,
            notif,
            search: SearchView {
                query: &search_text,
                selection: search_selection,
                focused: search_open,
                results: &self.search.results,
                selected: self.search.selected,
                details: self.search.details.known(),
                indexing: self.search.walking && !self.search.walked,
            },
            progress: track_progress(state, now),
            robot,
            eyes: self.eyes,
            eyes_pose: self.eyes_pose(),
            eq_heights: self.eq_heights(),
            robot_fades: self.eyes_morph,
            system: Some(&system),
            cleaner: cleaner.as_ref(),
            shot: shot::shot_view(&self.shots, &self.drawn),
            clip_notice: clip::clip_notice_view(&self.clip.history, &self.drawn),
            video_notice: self.screen.video_notice(),
            toast_logo: notif_page::toast_logo_view(&self.notif.logos, &self.drawn),
            toast_slot_w,
            toast_slot: self
                .slot_keep
                .as_ref()
                .map(|n| notif_page::toast_badge_of(logos, n)),
            toast_dot: state.toast_dot_shown() && !self.eyes,
            strip_badge: strip
                .filter(|n| is_real_toast(n))
                .map(|n| notif_page::toast_badge_of(logos, n)),
            strip_motion: self
                .strip_motion
                .as_ref()
                .and_then(|m| m.motion.frame(now, STRIP_H)),
            notice_close: self.notice_close_view(now),
            transit,
            full_redraw,
            play: self.play_frame(now, shape),
        };
        let key = eq_key(self.draw_gen, &frame, shape, opacity);
        if eq_tick && !dirty::eq_draw_needed(self.eq_key.as_ref(), &key) {
            return;
        }
        let drawn = match self
            .renderer
            .draw(win, self.config.style, shape, &frame, opacity)
        {
            Ok(()) => true,
            Err(error) => {
                eprintln!("vale: draw failed: {error}");
                false
            }
        };
        self.eq_key = drawn.then_some(key);
        // The IME composes where the panel drew the caret.
        if focused && let (Some(input), Some(caret)) = (input, self.renderer.cli_caret()) {
            input.set_ime_caret(caret);
        }
        if clip_focused && let (Some(input), Some(caret)) = (search, self.renderer.clip_caret()) {
            input.set_ime_caret(caret);
        }
        if search_open
            && let (Some(input), Some(caret)) = (search_field, self.renderer.search_caret())
        {
            input.set_ime_caret(caret);
        }
        // The regions are this frame's now: a tip whose target moved or went
        // goes with it.
        self.tip_check();
        // The ✕ may have just been registered under a resting pointer: light
        // its disc now instead of at the next move. Settles in one more frame
        // (the two then agree), so it cannot loop.
        if let Some(ptr) = self.close_ptr {
            let hot = self.hit_at(ptr) == Some(Hit::NoticeClose);
            if hot != self.close_hot && self.close_fade.alpha(now) > 0.0 {
                self.close_hot = hot;
                self.draw(now);
            }
        }
    }
}

/// The progress line is drawn by the compact view only: elsewhere a moving
/// position must not defeat the equalizer tick's skip.
fn key_progress(compact: bool, progress: Option<Progress>) -> Option<Progress> {
    progress.filter(|_| compact)
}

/// What `frame` draws that can change between two equalizer ticks.
fn eq_key(draw_gen: u64, frame: &Frame, shape: ShapeRect, opacity: f32) -> EqKey {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let hash = |text: &str| {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        h.finish()
    };
    let compact = *frame.view == View::MediaCompact;
    let playing = frame.media.is_some_and(|m| m.playing);
    // The renderer draws the sine (the clock's) for compact media playing
    // without live bars.
    let time_based = eq_time_based(
        compact,
        playing,
        frame.eq_heights.is_some(),
        frame.play.is_some(),
    );
    EqKey {
        draw_gen,
        shape: [shape.x, shape.y, shape.w, shape.h, shape.radius].map(f32::to_bits),
        opacity: opacity.to_bits(),
        eq_heights: frame
            .eq_heights
            .filter(|_| compact)
            .map(|h| h.map(f32::to_bits)),
        time_based,
        robot: frame.robot,
        progress: key_progress(compact, frame.progress),
        lyric: frame
            .lyric
            .map(|l| (hash(l.text), l.prev.map(hash), l.fade.to_bits())),
        lyric_scroll: [frame.lyric_scroll, frame.lyric_prev_scroll].map(f32::to_bits),
        fill_alpha: frame.fill_alpha.to_bits(),
        toast_slot_w: frame.toast_slot_w.to_bits(),
        toast_dot: frame.toast_dot,
        volume: frame.volume.map(f32::to_bits),
        notice_close: frame.notice_close,
        clock: frame.clock.clone(),
        view: frame.view.clone(),
    }
}

/// Pointer tracking for hover-to-expand. `TrackMouseEvent` is armed on the
/// first move inside the window; `WM_MOUSEHOVER` (after `HOVER_DWELL_MS`
/// without leaving the hover rectangle) expands, so a pointer just passing
/// over does not. `WM_MOUSELEAVE` ends tracking, so the next move re-arms it.
#[derive(Default)]
struct HoverTracking {
    /// Hover and leave tracking is armed for the pointer's current visit.
    armed: bool,
    /// `Hover(true)` was sent for this visit.
    hovered: bool,
    /// The ✕ of a notice closed it before the dwell: this visit never sends
    /// `Hover(true)`, so the panel does not open by hover under the pointer
    /// (`close_quiet`, cleared when the pointer leaves).
    quiet: bool,
}

impl HoverTracking {
    fn needs_arming(&self) -> bool {
        !self.armed
    }

    /// A dwell that landed outside the island: the next move over it arms
    /// the tracking again.
    fn disarm(&mut self) {
        self.armed = false;
    }

    fn armed(&mut self) {
        self.armed = true;
    }

    /// `Hover(true)` the first time the pointer dwells during a visit; never
    /// after the ✕ closed a notice in it.
    fn on_hover(&mut self) -> Option<Event> {
        if self.quiet {
            return None;
        }
        (!std::mem::replace(&mut self.hovered, true)).then_some(Event::Hover(true))
    }

    /// The ✕ closed a notice: no hover for the rest of this visit.
    fn close_quiet(&mut self) {
        self.quiet = true;
    }

    /// `Hover(false)` when this visit expanded the island.
    fn on_leave(&mut self) -> Option<Event> {
        self.armed = false;
        self.quiet = false;
        std::mem::take(&mut self.hovered).then_some(Event::Hover(false))
    }
}

/// Mouse-button presses seen by polling, for the click outside a kept panel.
/// A press is an up-to-down edge between two polls, or the "pressed since
/// the last read" bit (a click quicker than the poll). A press the island
/// took itself (its own button-down message, handled before the timer's) is
/// not outside, wherever the pointer is by the time of the poll.
#[derive(Default)]
struct OutsidePress {
    down: bool,
    inside: bool,
}

impl OutsidePress {
    /// A button went down on the island: the next press the poll sees is
    /// that one.
    fn press_inside(&mut self) {
        self.inside = true;
    }

    /// True on the poll that sees a press outside the island.
    fn poll(&mut self, down: bool, tapped: bool) -> bool {
        let pressed = (down && !self.down) || tapped;
        self.down = down;
        if !pressed {
            // A press the poll never saw (its bit spent elsewhere) must
            // not excuse the next one.
            if !down {
                self.inside = false;
            }
            return false;
        }
        !std::mem::take(&mut self.inside)
    }

    /// Starts from `down`: a press that began before the polling did is not
    /// an edge.
    fn reset(&mut self, down: bool) {
        self.down = down;
        self.inside = false;
    }
}

/// The outside-click poll's slow pace, once no input has come for
/// `OUTSIDE_IDLE_MS`.
const OUTSIDE_SLOW_MS: u32 = 120;
const OUTSIDE_IDLE_MS: u32 = 1_000;

/// The outside-click poll's interval: quick while the user is at the mouse
/// or keyboard, slow after a second without input.
fn outside_poll_ms(input_idle_ms: u32) -> u32 {
    if input_idle_ms > OUTSIDE_IDLE_MS {
        OUTSIDE_SLOW_MS
    } else {
        OUTSIDE_MS
    }
}

/// Milliseconds since the last keyboard or mouse input in this session;
/// `None` when Windows cannot say.
fn input_idle() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        Some(unsafe { GetTickCount() }.wrapping_sub(info.dwTime))
    } else {
        None
    }
}

/// `input_idle`, with 0 (just now) when it cannot be read: the outside poll
/// then stays quick.
fn input_idle_ms() -> u32 {
    input_idle().unwrap_or(0)
}

/// How late Windows may fire a polling timer to batch it with other
/// wake-ups: up to 30 ms, never more than half the interval.
fn timer_slack(ms: u32) -> u32 {
    (ms / 2).min(30)
}

/// (Any mouse button is down, any went down since the last read.) Left,
/// right and middle, so a swapped button pair makes no difference.
fn mouse_buttons() -> (bool, bool) {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .iter()
        .map(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16)
        .fold((false, false), |(down, tapped), s| {
            (down || s & 0x8000 != 0, tapped || s & 0x0001 != 0)
        })
}

/// Pairs a left press with its release: a click counts only when both land
/// on the island, on the same target, with nothing in between that ended the
/// press (the pointer leaving, the view changing, the window hiding).
#[derive(Default)]
struct ClickPair {
    /// What the pending press is on; `None` without one (or over nothing).
    pressed: Option<Hit>,
}

impl ClickPair {
    fn press(&mut self, hit: Option<Hit>) {
        self.pressed = hit;
    }

    /// The click, when the release is on what was pressed. Every release
    /// ends the press.
    fn release(&mut self, hit: Option<Hit>) -> Option<Hit> {
        let pressed = self.pressed.take()?;
        (hit? == pressed).then_some(pressed)
    }

    fn clear(&mut self) {
        self.pressed = None;
    }
}

/// The update download's percent changing on screen: the same notice with a
/// new number, not another view (no entrance fade, no cleared press).
fn progress_in_place(view: &View, drawn: &View) -> bool {
    matches!(
        (view, drawn),
        (View::Notice(a), View::Notice(b))
            if matches!(a.kind, NoticeKind::UpdateProgress { .. })
                && matches!(b.kind, NoticeKind::UpdateProgress { .. })
    )
}

/// A pending press ends when the island hides, another view shows, or the
/// notch swapped with the eyes (`eyes_swap`): its release lands on something
/// else, even at the same spot.
fn press_ends(view: &View, drawn: &View, eyes_swap: bool) -> bool {
    *view == View::Hidden || view != drawn || eyes_swap
}

/// The lyric's crossfade and scroll run (and need frames) only while the
/// lyric view shows it: not while the notch is shrunk to the eyes.
fn lyric_moves(view: &View, eyes: bool) -> bool {
    *view == View::MediaLyric && !eyes
}

/// A middle release plays or pauses only when its press was on the island
/// too, gestures still count, and the release is on a drawn island pixel.
fn middle_click(pressed_on_island: bool, gestures: bool, released_on_island: bool) -> bool {
    pressed_on_island && gestures && released_on_island
}

/// The key `GetAsyncKeyState` must read for the logical left (primary)
/// button: it reads the physical buttons, so with the pair swapped in
/// Settings the primary one is the physical right.
fn primary_button_vk(swapped: bool) -> VIRTUAL_KEY {
    if swapped { VK_RBUTTON } else { VK_LBUTTON }
}

/// An ordinary window (not the desktop, not click-through, not Vale's own)
/// lies under the normal notch `normal` (logical px in the island's window
/// at screen `origin`, `scale` physical px per logical px).
fn window_under_notch(origin: (i32, i32), scale: f32, normal: ShapeRect) -> bool {
    let bounds = crate::capture::Rect::new(
        origin.0 + (normal.x * scale).floor() as i32,
        origin.1 + (normal.y * scale).floor() as i32,
        (normal.w * scale).ceil() as i32,
        (normal.h * scale).ceil() as i32,
    );
    !crate::capture::win::top_windows(bounds).is_empty()
}

/// Whether the logical left (primary) button is down right now, the one that
/// sends `WM_LBUTTONDOWN`.
fn left_button_down() -> bool {
    let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
    let vk = primary_button_vk(swapped);
    (unsafe { GetAsyncKeyState(i32::from(vk.0)) }) as u16 & 0x8000 != 0
}

/// After the wheel turns a page, the Music page's cover leaves the wheel to
/// the pages this long: a paging scroll that passes over it keeps paging.
const PAGING_HOLD: Duration = Duration::from_millis(600);

/// The wheel turned a page less than `PAGING_HOLD` ago.
fn paging_recent(last_paging: Option<Instant>, now: Instant) -> bool {
    last_paging.is_some_and(|at| now.saturating_duration_since(at) < PAGING_HOLD)
}

/// Whether the wheel turns the music's volume rather than the pages: the
/// setting is on and a song plays, over the compact notch (both views of
/// it) or the Music page's cover, unless that page is in the middle of a
/// paging scroll. Anywhere else the wheel turns pages.
fn volume_wheel_applies(
    enabled: bool,
    playing: bool,
    view: &View,
    over_art: bool,
    paging: bool,
) -> bool {
    enabled
        && playing
        && match view {
            View::MediaCompact | View::MediaLyric => true,
            View::Panel(Page::Media) => over_art && !paging,
            _ => false,
        }
}

/// A mouse message's client position, in physical pixels.
fn client_xy(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam.0 & 0xFFFF) as i16 as i32,
        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}

/// The song a gesture asks for, as the button that would ask for it.
fn gesture_hit(dir: gestures::Dir) -> Hit {
    match dir {
        gestures::Dir::Prev => Hit::Prev,
        gestures::Dir::Next => Hit::Next,
    }
}

/// Shape and opacity morphing from `from` to `to` over `ms`: `ANIM_MS`
/// between views, `LYRIC_RESIZE_MS` for the lyric notch's width alone.
#[derive(Clone, Copy)]
struct Anim {
    from: ShapeRect,
    from_opacity: f32,
    to: ShapeRect,
    to_opacity: f32,
    start: Instant,
    ms: u64,
    running: bool,
    /// How the shape eases: the spring, a plain ease-out (Windows' animations
    /// off) or in and out for the eyes (`motion::morph_timing`).
    curve: MorphCurve,
    /// The content of the compact view or notice this morph leads to fades
    /// in with it, from this alpha (`motion::content_alpha_from`); `None`:
    /// no fade.
    fade_from: Option<f32>,
}

impl Anim {
    fn settled(shape: ShapeRect, opacity: f32) -> Self {
        Self {
            from: shape,
            from_opacity: opacity,
            to: shape,
            to_opacity: opacity,
            start: Instant::now(),
            ms: ANIM_MS,
            running: false,
            curve: MorphCurve::Spring,
            fade_from: None,
        }
    }

    /// The progress the content fade follows: the eyes' eased progress, so
    /// the content moves with the shape; the linear one for the other morphs.
    fn fade_t(&self, now: Instant) -> f32 {
        let t = self.progress(now);
        if self.curve == MorphCurve::EyesInOut {
            ease_in_out_cubic(t)
        } else {
            t
        }
    }

    /// The content's alpha while its fade-in is still running, else `None`.
    fn fading(&self, now: Instant) -> Option<f32> {
        let from = self
            .fade_from
            .filter(|_| self.running && self.progress(now) < 1.0)?;
        Some(content_alpha_from(from, self.fade_t(now)))
    }

    fn progress(&self, now: Instant) -> f32 {
        if !self.running {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        (elapsed / self.ms.max(1) as f32).min(1.0)
    }

    fn frame(&self, now: Instant) -> (ShapeRect, f32) {
        let t = self.progress(now);
        if t >= 1.0 {
            return (self.to, self.to_opacity);
        }
        let shape = lerp_shape(self.from, self.to, self.curve.ease(t));
        let opacity = self.from_opacity + (self.to_opacity - self.from_opacity) * t;
        (shape, opacity)
    }
}

/// A busy toast's slot share of a compact notch's width (0 to
/// `TOAST_SLOT_EXTRA`) over a morph: what showed when it began and what the
/// target has. Kept apart from the shape's width, which also changes with a
/// longer lyric line or another view: a slot present at both ends stays
/// whole the whole morph, and only a slot that comes or goes eases with it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SlotShare {
    from: f32,
    to: f32,
}

impl SlotShare {
    /// The share at `now`, following the shape's eased progress (an
    /// overshoot is clamped).
    fn at(&self, anim: &Anim, now: Instant) -> f32 {
        let t = anim.progress(now);
        if t >= 1.0 {
            return self.to;
        }
        (self.from + (self.to - self.from) * anim.curve.ease(t)).clamp(0.0, TOAST_SLOT_EXTRA)
    }
}

/// The lyric notch's width, worked out per line, and the line's scroll.
/// Reset when the lyric view closes, so the next one starts at its first
/// line's width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct LyricFit {
    /// The notch's width; `None` before the first line.
    width: Option<f32>,
    /// When the line on screen showed, and its measured width (`None`: the
    /// rest sign or nothing).
    since: Option<Instant>,
    text_w: Option<f32>,
    /// The line before's scroll when it was replaced, frozen while it fades.
    prev_scroll: f32,
    /// The robot and the cap `width` was worked out with.
    robot: bool,
    max_w: f32,
    /// The user's notch width, the notch's floor (`lyric_width_min`); 0 until
    /// set, which `lyric_width_min` reads as `LYRIC_MIN_W`.
    min_w: f32,
    /// A busy toast's logo sits at the notch's right (`TOAST_SLOT_EXTRA`):
    /// the width, the line's box and its scroll are worked out with it.
    slot: bool,
}

impl LyricFit {
    /// The user's notch width changed: the same line is fitted to the new
    /// floor.
    fn set_min(&mut self, min_w: f32) {
        if self.min_w == min_w {
            return;
        }
        self.min_w = min_w;
        if self.width.is_some() {
            self.width = Some(lyric_width_min(self.text_w, self.robot, min_w, self.max_w));
        }
    }

    /// A new line on screen at `now`: the notch grows to it at once, and
    /// shrinks only when it is `LYRIC_SHRINK_SLACK` or more narrower.
    fn line(&mut self, text_w: Option<f32>, now: Instant, robot: bool, max_w: f32) {
        self.prev_scroll = self.scroll(now);
        let wanted = lyric_width_min(text_w, robot, self.min_w, max_w);
        let current = self
            .width
            .filter(|_| self.robot == robot && self.max_w == max_w);
        // The cap never sits under the notch width the user set, and it
        // makes room for the robot's extra lead like `lyric_width_min`.
        let cap = lyric_cap(robot, self.min_w, max_w);
        self.width = Some(lyric_target_w(current, wanted).min(cap));
        self.since = Some(now);
        self.text_w = text_w;
        self.robot = robot;
        self.max_w = max_w;
    }

    /// The robot was turned on or off, or the screen changed: the same line
    /// is fitted again.
    fn refit(&mut self, robot: bool, max_w: f32) {
        if self.width.is_some() && (self.robot != robot || self.max_w != max_w) {
            self.width = Some(lyric_width_min(self.text_w, robot, self.min_w, max_w));
            self.robot = robot;
            self.max_w = max_w;
        }
    }

    /// The notch's width, the logo's room included while the slot shows.
    fn width(&self) -> f32 {
        let w = self.line_side_w();
        if self.slot {
            lyric_slot_width(w, self.robot, self.min_w, self.max_w)
        } else {
            w
        }
    }

    /// The width without the slot: what the line wanted.
    fn line_side_w(&self) -> f32 {
        self.width.unwrap_or(LYRIC_MIN_W.max(self.min_w))
    }

    /// The width the line's side keeps with the slot settled: the notch's
    /// width less the logo's room.
    fn content_w(&self) -> f32 {
        if self.slot {
            self.width() - TOAST_SLOT_EXTRA
        } else {
            self.width()
        }
    }

    /// The slot appeared or went: the width, the box and the scroll follow.
    fn set_slot(&mut self, on: bool) {
        self.slot = on;
    }

    fn box_w(&self) -> f32 {
        lyric_box_w(self.content_w(), self.robot)
    }

    /// The line's scroll at `now` (0 while it holds or when it fits).
    fn scroll(&self, now: Instant) -> f32 {
        match (self.since, self.text_w) {
            (Some(since), Some(text_w)) => {
                let ms = now.saturating_duration_since(since).as_millis() as u64;
                lyric_scroll_offset(text_w, self.box_w(), ms)
            }
            _ => 0.0,
        }
    }

    /// When a line too long for the notch starts and stops scrolling;
    /// `None` for one that fits.
    fn scroll_span(&self) -> Option<(Instant, Instant)> {
        let since = self.since?;
        let end = lyric_scroll_end_ms(self.text_w?, self.box_w())?;
        Some((
            since + Duration::from_millis(LYRIC_HOLD_MS),
            since + Duration::from_millis(end),
        ))
    }
}

/// A busy toast's strip moving in or out of the open panel: when it began
/// and which way, and (leaving) the notice it still draws while it fades.
#[derive(Clone, Debug, PartialEq)]
struct StripAnim {
    motion: StripMotion,
    leaving: Option<Notice>,
}

/// A panel page switch: `page_slide` eases from `strip.from` to 0 over
/// `SLIDE_MS`, the pages going out moving with it.
#[derive(Clone, Copy)]
struct Slide {
    strip: SlideStrip,
    start: Instant,
}

/// Two panel pages cross-fading (the search over a page, or back):
/// `progress` runs 0..1 over `PAGE_FADE_MS`.
#[derive(Clone, Copy)]
struct PageFade {
    /// The page it fades away from, drawn under the new one.
    from_page: Page,
    start: Instant,
}

impl PageFade {
    fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        (elapsed / PAGE_FADE_MS as f32).min(1.0)
    }
}

/// +1 when `after` comes later than `before` in the owner's page order
/// (it slides in from the right), else -1.
fn slide_from(pages: &PanelPages, before: Page, after: Page) -> f32 {
    if pages.position(after) > pages.position(before) {
        1.0
    } else {
        -1.0
    }
}

impl Slide {
    fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        (elapsed / SLIDE_MS).min(1.0)
    }

    fn offset(&self, now: Instant) -> f32 {
        let t = self.progress(now);
        // Cubic ease-out: fast start, gentle landing, no overshoot past the page.
        self.strip.from * (1.0 - t).powi(3)
    }
}

/// Whether the calendar should ask the sync thread for `range`. A different
/// range than the last one asked (navigation, or the panel opening on another
/// range) always asks. The same range asks again only once both the request
/// and the data shown for it (`fetched_age`, `None` if it is not that range or
/// the fetch failed) are older than `CALENDAR_FRESH`.
fn needs_fetch(
    asked: Option<((NaiveDate, NaiveDate), Instant)>,
    fetched_age: Option<Duration>,
    range: (NaiveDate, NaiveDate),
    now: Instant,
) -> bool {
    match asked {
        Some((last, at)) if last == range => {
            now.saturating_duration_since(at) >= CALENDAR_FRESH
                && fetched_age.is_none_or(|age| age >= CALENDAR_FRESH)
        }
        _ => true,
    }
}

/// Milliseconds until a clock that counts up changes its shown second
/// (`elapsed` rounded down), plus 1 ms so the redraw lands after the change.
fn ms_to_next_whole_second(elapsed: Duration) -> u32 {
    1000 - (elapsed.as_millis() % 1000) as u32 + 1
}

/// `TIMER_REC` runs only while the REC view shows a recording that is not
/// paused (a paused clock does not move) on a screen somebody can see.
/// Fullscreen is already out: `base_view` hides the view there.
fn rec_clock_timer_wanted(view_shown: bool, paused: bool, screen_away: bool) -> bool {
    view_shown && !paused && !screen_away
}

/// `TIMER_REC_LEVEL` adds Motion not Off to the same conditions: with it off
/// the meters stay at rest.
fn rec_level_timer_wanted(
    view_shown: bool,
    paused: bool,
    motion_on: bool,
    screen_away: bool,
) -> bool {
    rec_clock_timer_wanted(view_shown, paused, screen_away) && motion_on
}

/// Milliseconds until the countdown's shown second changes (it rounds up, so
/// at a whole second), plus 1 ms so the redraw lands after the change.
fn ms_to_next_second(remaining: Duration) -> u32 {
    match (remaining.as_millis() % 1000) as u32 {
        0 => 1001,
        ms => ms + 1,
    }
}

/// The fullscreen change to apply, if any. `reported` is what Windows says,
/// `own_overlay_up` that Vale's own pick overlay is on screen (it counts as a
/// fullscreen app to Windows, so nothing is learned while it is up) and
/// `last` the state already applied.
fn fullscreen_event(reported: bool, own_overlay_up: bool, last: bool) -> Option<bool> {
    if own_overlay_up || reported == last {
        None
    } else {
        Some(reported)
    }
}

/// Where the current track is now; `None` without media, a timeline or a
/// known length.
fn track_progress(state: &IslandState, now: Instant) -> Option<Progress> {
    let playing = state.media()?.playing;
    Progress::at(state.timeline(), playing, now)
}

/// How long until the Music page's elapsed time ticks over (just past the
/// position's next whole second); `None` (no timer) unless that page shows
/// a track that is playing and not yet at its end.
fn progress_wait(view: &View, progress: Option<Progress>, playing: bool) -> Option<u32> {
    let p = progress.filter(|p| p.position_ms < p.end_ms)?;
    (*view == View::Panel(Page::Media) && playing).then(|| 1001 - (p.position_ms % 1000) as u32)
}

/// Whether a drain of `received` events, `samples` of them system
/// readings, refreshes (and so redraws) the island: not when it held only
/// readings, which draw the open System page themselves, unless one
/// `raised` a notice.
fn drain_refreshes(received: usize, samples: usize, raised: bool) -> bool {
    received == 0 || samples < received || raised
}

/// The System page samples (and redraws) only while it shows, and not
/// while the cleaner covers it.
fn system_sample_wanted(view: &View, cleaner: bool) -> bool {
    *view == View::Panel(Page::System) && !cleaner
}

/// The System page's request counter: it counts only while the page shows, so
/// the next showing starts with a full reading, whenever it was left (even
/// before the first reply came back).
fn requests_while(wanted: bool, requests: u32) -> u32 {
    if wanted { requests } else { 0 }
}

/// What the System page's sampling does after a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PagePlan {
    /// Ask for a sample now (the page just showed, or its 1 s is up).
    Request,
    /// The page is gone: kill the one-shot.
    Stop,
    Keep,
}

/// `wanted`: the page shows; `timer`: the `LIVE_MS` one-shot is set;
/// `pending`: a sample is on its way (its arrival sets the timer).
fn system_page_plan(wanted: bool, timer: bool, pending: bool) -> PagePlan {
    match (wanted, timer, pending) {
        (true, false, false) => PagePlan::Request,
        (false, true, _) => PagePlan::Stop,
        _ => PagePlan::Keep,
    }
}

/// The next warning check: `SYSTEM_WARN_MS`, or sooner to confirm RAM's
/// minute above the limit (a little after it ends, at least 1 s).
fn warn_wait_ms(recheck: Option<Duration>) -> u32 {
    recheck.map_or(SYSTEM_WARN_MS, |d| {
        (d.as_millis() as u64 + 250).clamp(1_000, u64::from(SYSTEM_WARN_MS)) as u32
    })
}

/// When the island tries the morning greeting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GreetTrigger {
    /// The first calendar data after startup.
    Startup,
    /// Back from sleep. `by_user`: `PBT_APMRESUMESUSPEND`; only
    /// `PBT_APMRESUMEAUTOMATIC` also comes on unattended wakes (maintenance,
    /// Modern Standby).
    Wake { by_user: bool },
    /// `WTS_SESSION_UNLOCK`: someone just signed in to the screen.
    Unlock,
}

/// Whether `trigger` may greet while the session is `locked`. A locked
/// session shows the secure desktop, where nobody sees the greeting, and
/// Windows locks on wake by default: the day is kept for the unlock. A wake
/// that nobody asked for never greets.
fn greet_may_try(trigger: GreetTrigger, locked: bool) -> bool {
    match trigger {
        GreetTrigger::Unlock => true,
        GreetTrigger::Startup => !locked,
        GreetTrigger::Wake { by_user } => by_user && !locked,
    }
}

/// Whether Windows says this session is locked; `None` when it cannot tell.
fn query_session_locked() -> Option<bool> {
    let mut buffer = PWSTR::null();
    let mut bytes = 0u32;
    // SAFETY: on success Windows hands back a `WTSINFOEXW` it allocated,
    // read only within `bytes` and freed with `WTSFreeMemory`.
    let flags = unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
        .ok()?;
        if buffer.is_null() {
            return None;
        }
        let info = buffer.0 as *const WTSINFOEXW;
        let flags = (bytes as usize >= std::mem::size_of::<WTSINFOEXW>() && (*info).Level == 1)
            .then(|| (*info).Data.WTSInfoExLevel1.SessionFlags);
        WTSFreeMemory(buffer.0 as *mut c_void);
        flags?
    };
    match u32::try_from(flags).ok()? {
        WTS_SESSIONSTATE_LOCK => Some(true),
        WTS_SESSIONSTATE_UNLOCK => Some(false),
        _ => None,
    }
}

/// Messages after which the wall clock may have jumped.
/// `GUID_CONSOLE_DISPLAY_STATE`'s value: 0 off, 1 on, 2 dimmed (still on).
fn display_state_off(state: u8) -> bool {
    state == 0
}

/// From a `PBT_POWERSETTINGCHANGE` message: `Some(true)` when the display
/// went off, `Some(false)` when it is on; `None` for any other setting.
///
/// # Safety
/// `lparam` is that message's, a `POWERBROADCAST_SETTING` (or null).
unsafe fn display_off_from(lparam: LPARAM) -> Option<bool> {
    let setting = unsafe { (lparam.0 as *const POWERBROADCAST_SETTING).as_ref() }?;
    (setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && setting.DataLength >= 1)
        .then(|| display_state_off(setting.Data[0]))
}

fn is_clock_jump(msg: u32, wparam: WPARAM) -> bool {
    match msg {
        WM_TIMECHANGE => true,
        WM_POWERBROADCAST => matches!(wparam.0, PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC),
        _ => false,
    }
}

/// Events that count as activity for Bata's sleep clock: anything the user
/// or a source changed (media, notices, Claude, the CLI, clicks, the wheel,
/// the pointer arriving, editors). Timers, timelines and data refreshes do not.
fn is_activity(event: &Event) -> bool {
    matches!(
        event,
        Event::Media(_)
            | Event::Notice(_)
            | Event::ClaudeActivity { .. }
            | Event::Click(_)
            | Event::Wheel(_)
            | Event::PageFlick { .. }
            | Event::Hover(true)
            | Event::EditorOpen(_)
            | Event::CliFocus(_)
            | Event::CliScroll { .. }
            | Event::MoleLogScroll { .. }
            | Event::Cli(_)
            | Event::LyricsSetting(_)
    )
}

/// What `WM_RBUTTONUP` does next, once `on_right_click` has looked.
#[derive(Debug, PartialEq)]
enum RightClick {
    /// Handled (or ignored): nothing more.
    Done,
    /// The tray menu opens.
    Tray,
    /// The card menu opens for this note.
    NoteMenu(String),
    /// The transcript menu opens for this Rekam row.
    RecRowMenu(usize),
}

/// The note whose card a right-click at `hit` is on, on the Notes page.
fn note_card_menu_target(view: View, hit: Option<&Hit>) -> Option<String> {
    match hit {
        Some(Hit::NoteOpen(id) | Hit::NoteSticky(id)) if view == View::Panel(Page::Notes) => {
            Some(id.clone())
        }
        _ => None,
    }
}

/// The Rekam row whose menu a right-click at `hit` asks for: the row or its
/// pill, on the Rekam page only.
fn rec_row_menu_target(view: View, hit: Option<&Hit>) -> Option<usize> {
    match hit {
        Some(Hit::RecOpen(i) | Hit::RecTranscript(i)) if view == View::Panel(Page::Record) => {
            Some(*i)
        }
        _ => None,
    }
}

/// What a right-click does while the CLI folder menu is open.
#[derive(Debug, PartialEq)]
enum MenuRightClick {
    /// On a favourite's row: it is unstarred.
    Unstar(PathBuf),
    /// Elsewhere in the menu: nothing (no tray menu over the folder menu).
    Swallow,
    /// Outside the menu: the tray menu, as everywhere.
    Tray,
}

fn menu_right_click(hit: Option<Hit>) -> MenuRightClick {
    match hit {
        Some(Hit::CliFolderPick {
            path,
            favorite: true,
        }) => MenuRightClick::Unstar(path),
        Some(Hit::CliFolderPick { .. } | Hit::CliFolderStar { .. }) => MenuRightClick::Swallow,
        _ => MenuRightClick::Tray,
    }
}

/// The face a quiet robot shows in `view` with the robot `on` (for tests of
/// which timers run).
#[cfg(test)]
fn quiet_face(view: &View, on: bool) -> Option<robot::Expr> {
    let inputs = robot::RobotInputs {
        spot: Spot::of(view),
        hover: false,
        asleep: false,
        claude_working: false,
        cli: robot::Cli::None,
        mole_running: false,
        battery_low: false,
    };
    choose_expr(&inputs).filter(|_| on)
}

/// What the greeting for `period` counts at `now`:
/// - Morning: today's events and today's reminders not yet done.
/// - Midday and Afternoon: what is left today: events that have not ended
///   (all-day ones count) and undone reminders due at or after `now`.
/// - Evening: tomorrow's events (the date after `now`'s).
/// - Night (00:00-03:59): the coming day's events: the current calendar
///   date, since the user has not slept yet.
fn greeting_counts(
    events: &[CalEvent],
    reminders: &[Reminder],
    now: NaiveDateTime,
    period: DayPeriod,
) -> GreetCounts {
    let today = now.date();
    let due_today = |r: &&Reminder| {
        !r.done && reminders::local_time(r.due_unix).is_some_and(|t| t.date() == today)
    };
    match period {
        DayPeriod::Morning => GreetCounts {
            events: crate::calendar::events_on(events, today).len(),
            reminders: reminders.iter().filter(due_today).count(),
        },
        DayPeriod::Midday | DayPeriod::Afternoon => GreetCounts {
            events: crate::calendar::events_on(events, today)
                .into_iter()
                .filter(|ev| !event_over(ev, now))
                .count(),
            reminders: reminders
                .iter()
                .filter(due_today)
                .filter(|r| reminders::local_time(r.due_unix).is_some_and(|t| t >= now))
                .count(),
        },
        DayPeriod::Evening => GreetCounts {
            events: today
                .succ_opt()
                .map_or(0, |d| crate::calendar::events_on(events, d).len()),
            reminders: 0,
        },
        DayPeriod::Night => GreetCounts {
            events: crate::calendar::events_on(events, today).len(),
            reminders: 0,
        },
    }
}

/// A timed event whose end has passed at `now` (a zero-length one, when its
/// start has). An all-day event lasts the whole day.
fn event_over(ev: &CalEvent, now: NaiveDateTime) -> bool {
    match (&ev.start, &ev.end) {
        (crate::calendar::EventTime::At(s), crate::calendar::EventTime::At(e)) => {
            let (s, e) = (s.naive_local(), e.naive_local());
            if e <= s { s < now } else { e <= now }
        }
        _ => false,
    }
}

/// The shape and opacity to draw at `now` with a rise in progress: the
/// opacity multiplied by the rise's factor (a morph's own fade is kept), the
/// shape moved down by its offset (up to 0, so it starts higher). No rise, or
/// a finished one, leaves both as they are.
fn rise_frame(
    shape: ShapeRect,
    opacity: f32,
    rise: Option<&behind::Rise>,
    now: Instant,
) -> (ShapeRect, f32) {
    match rise {
        Some(rise) if rise.running(now) => {
            let (factor, dy) = rise.at(now);
            (
                ShapeRect {
                    y: shape.y + dy,
                    ..shape
                },
                opacity * factor,
            )
        }
        _ => (shape, opacity),
    }
}

/// `TIMER_EQ` (33 ms) runs only while the compact view shows playing media
/// with the full notch, on a screen that is not fullscreen, locked or off.
/// The lyric view runs it too, but only while the cat dances there
/// (`dance_wanted`): it is the tick that redraws the nod.
fn eq_timer_wanted(
    view_compact: bool,
    view_lyric: bool,
    dance_wanted: bool,
    playing: bool,
    eyes: bool,
    fullscreen: bool,
    screen_away: bool,
) -> bool {
    (view_compact || (view_lyric && dance_wanted))
        && playing
        && !eyes
        && !fullscreen
        && !screen_away
}

/// The capture thread runs exactly while `TIMER_EQ` does and "Equalizer ikut
/// irama" is on.
fn eq_capture_wanted(timer_on: bool, setting_on: bool) -> bool {
    timer_on && setting_on
}

/// The idle clock's minute timer runs only while the idle island shows it.
/// What a story was started for, and how the island looked then: a style,
/// size or mascot that differs later ends it.
struct ActivePlay {
    play: Play,
    moment: Moment,
    shape: PlayShape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PlayShape {
    style: Style,
    size: NotchSize,
    mascot: Mascot,
}

fn play_shape_of(style: Style, size: NotchSize, mascot: Mascot) -> PlayShape {
    PlayShape { style, size, mascot }
}

/// `TIMER_MASCOT_PLAY` should be set: an idle story could start, apart from
/// the pointer (read when it fires), the presence rule and the cooldown, and
/// none runs. Only the timed presets have a wait.
fn mascot_timer_wanted(env: &Env, preset: MascotPlay, playing: bool) -> bool {
    let env = Env {
        pointer_near: false,
        ..*env
    };
    !playing
        && wait_for(preset, 0).is_some()
        && may_start(&env, Moment::Idle, preset, PRESENCE_MIN, cooldown(preset))
}

/// The one-shot to arm (`Some(true)`) or kill (`Some(false)`) so it matches
/// `wanted`; `None` leaves it as it is, so a set timer is never set again.
fn timer_change(armed: bool, wanted: bool) -> Option<bool> {
    (armed != wanted).then_some(wanted)
}

/// One frame of a story. Where the motion itself is unwanted it lands at rest
/// at once; where a gate stops holding it eases home. True when it is done.
fn step_play(
    env: &Env,
    play: &mut Play,
    moment: Moment,
    shape_changed: bool,
    clock_jumped: bool,
    now: Instant,
) -> bool {
    if instant_stop(env) {
        play.finish_now();
    } else if must_stop(env, play.story(), moment, shape_changed, clock_jumped) {
        play.end(now);
    }
    play.done(now)
}

/// What one frame of a running story leaves behind.
struct Advance {
    /// The story that was just dropped.
    finished: Option<Story>,
    /// The island window should pass clicks through its pixels.
    click_through: bool,
}

/// One frame of the story in `slot`: a mouse button down ends it, `step_play`
/// runs, and a finished story is dropped. The click-through state is derived
/// from the slot after all that, so the frame that drops a story, by any path,
/// is the frame that clears it.
fn advance_slot(
    slot: &mut Option<ActivePlay>,
    env: &Env,
    shape: PlayShape,
    clock_jumped: bool,
    clicked: bool,
    inside: bool,
    now: Instant,
) -> Advance {
    let mut finished = None;
    if let Some(active) = slot.as_mut() {
        if clicked {
            active.play.end(now);
        }
        let changed = active.shape != shape;
        if step_play(env, &mut active.play, active.moment, changed, clock_jumped, now) {
            finished = Some(active.play.story());
            *slot = None;
        }
    }
    Advance {
        finished,
        click_through: click_through(slot.is_some(), inside),
    }
}

/// What `play_when_settled` does: start now (no morph; `try_play` still checks
/// the gates), wait out the morph's remaining time, or nothing.
#[derive(Debug, PartialEq)]
enum Settle {
    Now,
    Wait(Duration),
    Never,
}

/// `left`: the morph's remaining time, if one runs. `could`: the play may
/// start at all. The wait is one millisecond past the morph's end.
fn settle_plan(left: Option<Duration>, could: bool) -> Settle {
    match left {
        None => Settle::Now,
        Some(left) if could => Settle::Wait(left + Duration::from_millis(1)),
        Some(_) => Settle::Never,
    }
}

/// How long the island's morph still has to run at `now`; `None` once it has
/// reached its shape. A deferred play (greeting, menu pick) waits for that.
fn morph_left(running: bool, start: Instant, ms: u64, now: Instant) -> Option<Duration> {
    let end = start + Duration::from_millis(ms);
    (running && now < end).then(|| end - now)
}

/// The time since the last story ended; a first story has no cooldown.
fn since_last_play(last: Option<Instant>, now: Instant) -> Duration {
    last.map_or(Duration::MAX, |at| now.saturating_duration_since(at))
}

/// The clock moved on to another hour.
fn hour_rolled(shown: u32, now: u32) -> bool {
    shown != now
}

/// A greeting notice was on screen at the last call and is not now.
fn greeting_ended(was_up: &mut bool, view: &View) -> bool {
    let up = matches!(
        view,
        View::Notice(n) if matches!(n.kind, NoticeKind::Greeting { .. })
    );
    std::mem::replace(was_up, up) && !up
}

/// The frame changes between two equalizer ticks without the equalizer: the
/// sine of compact media playing with no live bars, and a story.
fn eq_time_based(compact: bool, playing: bool, live_bars: bool, play: bool) -> bool {
    (compact && playing && !live_bars) || play
}

fn clock_timer_wanted(view: &View, idle_clock: bool) -> bool {
    idle_clock && *view == View::Idle
}

/// Milliseconds from `now` until the shown minute changes, plus 1 ms so the
/// redraw lands after the change. A timer that fires early just redraws the
/// same minute and re-arms for the few milliseconds left.
fn ms_to_next_minute(now: NaiveTime) -> u32 {
    // A leap second reports 1_000_000_000 ns and more; it counts as the 59th.
    let ms = now.second() * 1000 + (now.nanosecond() / 1_000_000).min(999);
    60_000 - ms + 1
}

/// False when saving would change nothing: an existing note with the same
/// text, or an empty new one.
fn note_changed(notes: &NotesStore, id: Option<&str>, text: &str) -> bool {
    match id.and_then(|id| notes.get(id)) {
        Some(note) => note.text != text,
        None => !text.trim().is_empty(),
    }
}

/// The note editor for `id`; an unknown id (deleted meanwhile) opens a new note.
fn note_editor_kind(notes: &NotesStore, id: Option<&str>) -> EditorKind {
    let note = id.and_then(|id| notes.get(id));
    EditorKind::Note {
        id: note.map(|n| n.id.clone()),
        text: note.map(|n| n.text.clone()).unwrap_or_default(),
    }
}

/// The event editor for `event_id` (read-only unless the event is editable),
/// or a new event on `date`. `None` when the event is no longer in the data.
fn event_editor_kind(
    events: &[CalEvent],
    event_id: Option<&str>,
    date: NaiveDate,
) -> Option<EditorKind> {
    let Some(id) = event_id else {
        return Some(EditorKind::Event {
            event_id: None,
            calendar_id: None,
            draft: new_draft(date),
            read_only: false,
        });
    };
    let ev = events.iter().find(|e| e.id == id)?;
    Some(EditorKind::Event {
        event_id: Some(ev.id.clone()),
        calendar_id: Some(ev.calendar_id.clone()),
        draft: draft_from(ev),
        read_only: !ev.editable,
    })
}

/// Values table: Windows' default notification sound, once, without blocking;
/// silent if the sound scheme has none.
fn play_phase_sound() {
    unsafe {
        let _ = PlaySoundW(
            w!("Notification.Default"),
            None,
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

/// Frame intervals for measuring smoothness, on when `DI_FRAME_LOG=1`. Each
/// animation's intervals (ms, the first one measured from its start) are kept
/// in memory and appended to `%TEMP%\di-frames.log` under a marker line once
/// it ends, so logging adds no file I/O between frames.
struct FrameLog {
    path: Option<PathBuf>,
    /// Time of the previous frame while an animation is being logged.
    last: Option<Instant>,
    intervals: Vec<f32>,
}

impl FrameLog {
    fn from_env() -> Self {
        let on = std::env::var_os("DI_FRAME_LOG").is_some_and(|v| v == "1");
        Self {
            path: on.then(|| std::env::temp_dir().join("di-frames.log")),
            last: None,
            intervals: Vec::new(),
        }
    }

    /// An animation starts (or retargets, which ends the previous one).
    fn start(&mut self, now: Instant) {
        if self.path.is_some() {
            self.finish();
            self.last = Some(now);
        }
    }

    fn frame(&mut self, now: Instant) {
        let Some(last) = self.last else { return };
        let ms = now.saturating_duration_since(last).as_secs_f32() * 1000.0;
        self.intervals.push(ms);
        self.last = Some(now);
    }

    /// Appends the current animation's intervals; a write failure only loses the log.
    fn finish(&mut self) {
        let (Some(path), Some(_)) = (&self.path, self.last.take()) else {
            return;
        };
        let mut text = String::from("# animation\n");
        for ms in self.intervals.drain(..) {
            let _ = writeln!(text, "{ms:.2}");
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(text.as_bytes());
        }
    }
}

/// A temporary notice from the island itself.
fn island_notice(key: &str, title: &str, body: &str) -> Notice {
    Notice {
        key: format!("vale-{key}"),
        source: "Vale".into(),
        title: title.into(),
        body: body.into(),
        kind: NoticeKind::App {
            app_id: None,
            toast: None,
        },
    }
}

/// A sticky note's reminder went off: the note's title over its time, with
/// "Tunda 10 mnt" and "Buka". Sticky, one per note.
fn sticky_reminder_notice(id: &str, title: &str, body: &str) -> Notice {
    Notice {
        key: format!("vale-sticky-remind-{id}"),
        source: "Vale".into(),
        title: title.into(),
        body: body.into(),
        kind: NoticeKind::StickyReminder { id: id.to_owned() },
    }
}

/// At startup: notes.json exists but could not be read or parsed.
fn notes_unreadable_notice(notes: &NotesStore) -> Option<Notice> {
    notes.load_failed().then(|| {
        island_notice(
            "notes-unreadable",
            tr(Key::NotesUnreadable),
            tr(Key::NotesUnreadableBody),
        )
    })
}

/// At startup: reminders.json exists but could not be read or parsed.
fn reminders_unreadable_notice(reminders: &RemindersStore) -> Option<Notice> {
    reminders.load_failed().then(|| {
        island_notice(
            "reminders-unreadable",
            tr(Key::RemindersUnreadable),
            tr(Key::RemindersUnreadableBody),
        )
    })
}

/// The sticky notice for a reminder whose time came: its text over the time
/// ("09.30", or "26 Okt 09.30" on another day), marked "Terlewat · " when it
/// is announced late: at startup, or over a minute after its time.
fn reminder_notice(r: &Reminder, now: i64, startup: bool) -> Notice {
    let lang = i18n::lang();
    let when = match (
        reminders::local_time(r.due_unix),
        reminders::local_time(now),
    ) {
        (Some(due), Some(today)) if due.date() == today.date() => i18n::time_hm(lang, due),
        (Some(due), _) => format!(
            "{} {}",
            i18n::short_date(lang, due.date()),
            i18n::time_hm(lang, due)
        ),
        (None, _) => String::new(),
    };
    let late = startup || now - r.due_unix > REMINDER_LATE_SECS;
    let body = if late {
        format!("{} · {when}", i18n::text(lang, Key::Missed))
    } else {
        when
    };
    Notice {
        key: format!("reminder-{}", r.id),
        source: tr(Key::SourceReminder).into(),
        title: r.text.clone(),
        body,
        kind: NoticeKind::Reminder { id: r.id.clone() },
    }
}

/// Milliseconds until the reminder due at `next` (Unix seconds), plus 1 ms
/// so it is due when the timer fires; never more than `REMINDER_MAX_MS`.
/// `None` when nothing is pending.
fn reminder_wait_ms(next: Option<i64>, now_ms: i64) -> Option<u32> {
    let due_ms = next?.saturating_mul(1000);
    let wait = due_ms.saturating_sub(now_ms).max(0).saturating_add(1);
    Some(wait.min(REMINDER_MAX_MS) as u32)
}

/// The reminder editor for `id`; an unknown id (deleted meanwhile) or `None`
/// opens a new one-off reminder at the next whole hour after `now`.
fn reminder_editor_kind(
    reminders: &RemindersStore,
    id: Option<&str>,
    now: NaiveDateTime,
) -> EditorKind {
    let existing = id.and_then(|id| reminders.get(id));
    if let Some(r) = existing
        && let Some(due) = reminders::local_time(r.due_unix)
    {
        // A weekly one saved without days rings on its due date's weekday.
        let days = match (r.repeat, r.days.is_empty()) {
            (reminders::Repeat::Weekly, true) => vec![due.date().weekday()],
            _ => r.days.clone(),
        };
        return EditorKind::Reminder {
            id: Some(r.id.clone()),
            text: r.text.clone(),
            date: due.date(),
            // The time as set: on a day DST skips it, `due_unix` shows another.
            time: reminders.wall_time(&r.id).unwrap_or(due.time()),
            repeat: r.repeat,
            days,
        };
    }
    let next_hour =
        now.date().and_hms_opt(now.hour(), 0, 0).unwrap_or(now) + chrono::Duration::hours(1);
    EditorKind::Reminder {
        id: None,
        text: String::new(),
        date: next_hour.date(),
        time: next_hour.time(),
        repeat: reminders::Repeat::Once,
        days: Vec::new(),
    }
}

/// What "Hubungkan Google" does, given whether config.json is valid (or
/// absent) and the client ID in effect.
#[derive(Debug, PartialEq)]
enum ConnectAction {
    InvalidConfig,
    NoClientId,
    Login,
}

/// `client`: what `client::for_config` gives for the reloaded config.
fn connect_action(file_ok: bool, client: Option<Client>) -> ConnectAction {
    if !file_ok {
        ConnectAction::InvalidConfig
    } else if client.is_none() {
        ConnectAction::NoClientId
    } else {
        ConnectAction::Login
    }
}

/// config.json does not parse: after a save (`repair`, which says where it
/// went), or on "Hubungkan Google" (`None`, nothing moved yet).
fn invalid_config_notice(repair: Option<&Repair>) -> Notice {
    let body = match repair {
        Some(Repair::BackedUp(path)) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            i18n::config_copied_to(i18n::lang(), &name)
        }
        Some(Repair::Blocked) => tr(Key::ConfigNotCopied).into(),
        Some(Repair::None) | None => tr(Key::ConfigCheck).into(),
    };
    island_notice("config-invalid", tr(Key::ConfigInvalid), &body)
}

/// "Hubungkan Google" without a client ID in `config.json`.
fn client_id_notice() -> Notice {
    Notice {
        key: "calendar-client-id".into(),
        source: tr(Key::SourceCalendar).into(),
        title: tr(Key::ClientIdMissing).into(),
        body: tr(Key::ClientIdMissingBody).into(),
        kind: NoticeKind::App {
            app_id: None,
            toast: None,
        },
    }
}

fn dnd_notice() -> Notice {
    island_notice("dnd", tr(Key::DndTitle), tr(Key::DndBody))
}

/// Every start: asks for notification access when it is missing (a new
/// package identity starts without it, even after onboarding). On first run
/// the Do Not Disturb hint follows once the user has answered.
fn request_access_then_onboard(hwnd: HWND) {
    let asking = !notifications::access_allowed()
        && notifications::ask_access(poster(hwnd, WM_APP_ACCESS)).is_ok();
    if !asking {
        onboard_if_needed();
    }
}

/// The access prompt was answered. The poller started at launch exited
/// without access, so start it again if access is now granted.
fn on_access_answered() {
    if notifications::access_allowed() {
        with_app(App::spawn_notifications);
    }
    onboard_if_needed();
}

/// Shows the Do Not Disturb hint and Settings page once, on first run.
fn onboard_if_needed() {
    if with_app(|app| app.config.onboarding_done) == Some(false) {
        show_dnd_help();
        with_app(App::finish_onboarding);
    }
}

/// The Do Not Disturb hint as a notice, plus the Settings page it points to.
/// Settings opens outside `with_app`: the shell may pump messages meanwhile.
fn show_dnd_help() {
    with_app(|app| app.apply_now(Event::Notice(dnd_notice())));
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:notifications"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// The Notes card menu: one item, stick or unstick. Modal like `open_menu`,
/// so the app is borrowed only before and after it.
fn open_note_card_menu(hwnd: HWND, id: &str) {
    if MENU_OPEN.get() {
        return;
    }
    let Some(label) = with_app(|app| {
        let on = app.notes.get(id).is_some_and(|n| n.sticky.is_some());
        let key = if on {
            Key::StickyOffDesktopTip
        } else {
            Key::StickyOnDesktopTip
        };
        i18n::text(i18n::lang(), key)
    }) else {
        return;
    };
    MENU_OPEN.set(true);
    let chosen = tray::note_card_menu(hwnd, label);
    MENU_OPEN.set(false);
    // The same path as a click on the card's icon, so the panel redraws.
    if chosen {
        with_app(|app| app.apply_now(Event::Click(Hit::NoteSticky(id.to_owned()))));
    }
}

/// The device dropdown of `source` (from the Rekam page), under the device
/// name that was clicked: the endpoints are listed as it opens. It holds the
/// panel while it is open; its pick comes back as `WM_APP_MENU_DONE`
/// (`App::rec_menu_done`). Opened outside the app borrow: it takes the
/// foreground, which sends messages back here.
fn open_rec_device_menu(hwnd: HWND, source: crate::recorder::Source) {
    if MENU_OPEN.get() {
        return;
    }
    let Some(Some((current, lang, font, anchor))) = with_app(|app| {
        let anchor = app.rec_device_anchor(source)?;
        Some((
            app.config.recording.device(source).map(str::to_owned),
            app.config.language,
            app.config.font,
            anchor,
        ))
    }) else {
        return;
    };
    let opened = crate::ui::device_menu::open(
        hwnd,
        anchor,
        crate::ui::menu_pop::Align::Start,
        source,
        current.as_deref(),
        lang,
        font,
    );
    if opened {
        with_app(|app| app.apply_now(Event::MenuOpen(true)));
    }
}

/// The video quality menu of the Rekam page's Layar tab, under the quality
/// row. Its pick comes back as `WM_APP_MENU_DONE`. Opened outside the app
/// borrow, like the device menu.
fn open_rec_quality_menu(hwnd: HWND) {
    if MENU_OPEN.get() {
        return;
    }
    let Some(Some((current, lang, font, anchor))) = with_app(|app| {
        let anchor = app.rec_quality_anchor()?;
        let r = &app.config.recording;
        Some(((r.video_quality, r.video_fps), app.config.language, app.config.font, anchor))
    }) else {
        return;
    };
    let opened = crate::ui::quality_menu::open(
        hwnd,
        anchor,
        crate::ui::menu_pop::Align::Start,
        current,
        lang,
        font,
    );
    if opened {
        with_app(|app| app.apply_now(Event::MenuOpen(true)));
    }
}

/// The transcript menu of Rekam row `i`, under the row. A row with transcripts
/// off has no menu, and no tray menu either: the right-click is swallowed.
/// It holds the panel while it is open; its pick comes back as
/// `WM_APP_MENU_DONE`. Opened outside the app borrow, like the device menu.
fn open_rec_row_menu(hwnd: HWND, i: usize) {
    if MENU_OPEN.get() {
        return;
    }
    let Some(Some((audio, rows, font, anchor))) = with_app(|app| {
        let anchor = app.rec_row_anchor(i)?;
        let (audio, rows) = app.rec_row_menu(i)?;
        Some((audio, rows, app.config.font, anchor))
    }) else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    let opened = crate::ui::transcript_menu::open(
        hwnd,
        anchor,
        crate::ui::menu_pop::Align::End,
        audio,
        rows,
        font,
    );
    if opened {
        with_app(|app| app.apply_now(Event::MenuOpen(true)));
    }
}

/// The redo menu of a transcript window, under its "Transkrip ulang" button.
/// The pick comes back as `WM_APP_MENU_DONE` like the Rekam row's menu (the
/// island owns it); the activation goes back to the window when it closes.
fn open_transcript_redo_menu(audio: PathBuf, rows: Vec<crate::ui::menu_pop::Row>, anchor: crate::capture::Rect) {
    if MENU_OPEN.get() {
        return;
    }
    let Some((hwnd, font)) = with_app(|app| (app.hwnd, app.config.font)) else {
        return;
    };
    crate::ui::transcript_menu::open(
        hwnd,
        anchor,
        crate::ui::menu_pop::Align::Start,
        audio,
        rows,
        font,
    );
}

/// Shows the tray menu and runs the choice. The menu's modal loop dispatches
/// timer and mouse messages to `wnd_proc`, so no `APP` borrow is held across it.
fn open_menu(hwnd: HWND) {
    if MENU_OPEN.get() {
        return;
    }
    let Some((config, update)) = with_app(|app| {
        // A hotkey another app held may be free by now.
        app.retry_hotkeys();
        // Whether "Mainkan maskot" shows: not while a play runs.
        let env = tray::shut_panel_env(
            &app.play_env(Instant::now(), false),
            app.state.view_if_panel_shut() == View::Idle,
        );
        let play_item = tray::play_item_visible(&env, app.play.is_some());
        tray::set_play_item(play_item);
        (app.config.clone(), app.update.known_version())
    }) else {
        return;
    };
    MENU_OPEN.set(true);
    // The menu takes the foreground: the window that had the keyboard is
    // what "Cari" hands it back to on Esc.
    let before = unsafe { GetForegroundWindow() };
    // Listed now, outside the app borrow, so the Monitor submenu shows the
    // screens as they are.
    let displays = arrange_win::displays();
    let cmd = tray::show_menu(hwnd, &config, update.as_deref(), &displays);
    MENU_OPEN.set(false);
    let cmd = match cmd {
        Some(MenuChoice::Cmd(cmd)) => Some(cmd),
        // The same path as Settings: save, then move the island (outside
        // `with_app`), then bring an open Settings window up to date.
        Some(MenuChoice::Monitor(choice)) => {
            let changed = with_app(|app| app.set_notch_monitor(choice)).unwrap_or(false);
            if changed {
                reposition();
            }
            with_app(|app| app.sync_settings());
            None
        }
        None => None,
    };
    match cmd {
        Some(MenuCmd::PlayMascot) => {
            with_app(|app| app.on_play_mascot_menu());
        }
        Some(MenuCmd::CheckUpdate) => {
            with_app(|app| app.update_manual_check());
        }
        Some(MenuCmd::InstallUpdate) => {
            with_app(|app| app.update_install());
        }
        Some(MenuCmd::Settings) => open_settings(hwnd),
        // Whatever the hotkey is (off, taken): the menu always opens it
        // (and never closes it).
        Some(MenuCmd::Search) => {
            with_app(|app| app.on_search_menu(before));
        }
        Some(MenuCmd::StickyNote) => {
            with_app(|app| app.on_sticky_menu());
        }
        Some(MenuCmd::StickyList) => {
            with_app(|app| app.open_note_list(None));
        }
        Some(MenuCmd::Record) => {
            with_app(|app| app.rec_toggle());
        }
        Some(MenuCmd::RecordMute) => {
            with_app(|app| app.rec_toggle_mute(crate::recorder::Source::Mic));
        }
        Some(MenuCmd::RecordScreen) => {
            with_app(|app| app.screen_toggle());
        }
        Some(MenuCmd::Captions) => {
            with_app(|app| app.captions_toggle());
        }
        Some(MenuCmd::StylePill) => {
            with_app(|app| app.set_style(Style::Pill));
        }
        Some(MenuCmd::StyleNotch) => {
            with_app(|app| app.set_style(Style::Notch));
        }
        // Same path as the Settings toggle. The menu is rebuilt from the config
        // each time it opens, so a Settings change shows here without a sync.
        Some(MenuCmd::Autostart) => {
            with_app(|app| {
                let on = !app.config.start_with_windows;
                app.set_autostart(on);
                app.sync_settings();
            });
        }
        Some(MenuCmd::Exit) => shutdown(hwnd),
        None => {}
    }
}

/// Opens the settings window, or brings the open one forward. Outside
/// `with_app`: creating and focusing a window sends messages.
fn open_settings(hwnd: HWND) {
    let Some(model) = with_app(|app| {
        // Settings shows whether a hotkey is taken: try a failed one again.
        app.retry_hotkeys();
        app.settings_model()
    }) else {
        return;
    };
    if let Err(error) = settings::open(hwnd, model, None) {
        eprintln!("vale: cannot open settings: {error}");
    }
}

/// `open_settings`, on Fitur scrolled to `card`.
fn open_settings_card(hwnd: HWND, card: CardId) {
    let Some(model) = with_app(|app| {
        app.retry_hotkeys();
        app.settings_model()
    }) else {
        return;
    };
    if let Err(error) = settings::open_card(hwnd, model, card) {
        eprintln!("vale: cannot open settings: {error}");
    }
}

/// Runs a settings action now, or once the handler holding the app returns.
/// The Do Not Disturb help opens the Settings app, which may pump messages,
/// so it never runs inside `with_app`.
fn deliver_settings_action(action: SettingsAction) {
    if action == SettingsAction::DndHelp {
        show_dnd_help();
        return;
    }
    let mut pending = Some(action);
    with_app(|app| {
        if let Some(action) = pending.take() {
            app.on_settings_action(action);
        }
    });
    if let Some(action) = pending {
        DEFERRED_SETTINGS.with_borrow_mut(|queue| queue.push(action));
    }
}

/// `WM_APP_SCREEN_PICK`: hands the picker's answer to the screen recording
/// now, or once the handler holding the app returns. The pick stays where
/// the picker left it (this thread) until the app can take it.
fn deliver_screen_pick() {
    let taken = with_app(|app| {
        app.screen_pick_done(crate::capture::rec_pick::take_picked());
    });
    if taken.is_none() {
        DEFERRED_SCREEN_PICK.set(true);
    }
}

/// Closes the island. An open editor is closed first and its last result
/// (an edited note) handled here: once the island is gone, nobody reads it.
fn shutdown(hwnd: HWND) {
    shelf_ole::revoke(hwnd);
    settings::close();
    crate::transcript_win::close_all();
    flush_editor();
    // A command still running ends with the island's process, which closes
    // its job.
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

/// Closes an open editor now and handles its last result synchronously.
/// Called outside `with_app`: destroying the editor sends window messages.
fn flush_editor() {
    if let Some(result) = editor::close_open_editor() {
        deliver_result(result);
    }
}

/// Handles an editor result now, or once the handler holding the app returns.
fn deliver_result(result: EditorResult) {
    let mut pending = Some(result);
    with_app(|app| {
        if let Some(result) = pending.take() {
            app.on_editor_result(result);
        }
    });
    if let Some(result) = pending {
        DEFERRED_RESULTS.with_borrow_mut(|results| results.push(result));
    }
}

/// Editor results still queued for the island (an editor destroyed along
/// with it posts its last one); read before the queue goes with the window.
fn drain_editor_results(hwnd: HWND) {
    let mut msg = MSG::default();
    while unsafe {
        PeekMessageW(
            &mut msg,
            Some(hwnd),
            WM_APP_EDITOR_RESULT,
            WM_APP_EDITOR_RESULT,
            PM_REMOVE,
        )
    }
    .as_bool()
    {
        if let Some(result) = unsafe { editor::take_result(msg.lParam) } {
            deliver_result(result);
        }
    }
}

/// Runs `f` on the app, or returns `None` when it does not exist yet or is
/// already borrowed by a handler further up the stack.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    with_app_quiet(|app| {
        // Every message, event and input reaches the app here: the
        // equalizer tick's skip (`draw_eq_tick`) sees that something ran.
        app.draw_gen = app.draw_gen.wrapping_add(1);
        let result = f(app);
        // A handler may draw and then change what the closed view shows: the
        // key it stored must not match the next tick.
        app.draw_gen = app.draw_gen.wrapping_add(1);
        result
    })
}

/// `with_app` without the `draw_gen` bump: the equalizer tick alone.
fn with_app_quiet<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut app = cell.try_borrow_mut().ok()?;
        app.as_mut().map(f)
    })
}

/// Puts the island on the screen the saved choice resolves to (the primary one
/// by default) at that screen's current DPI, then redraws, and brings an open
/// Settings window up to date with the screens now. Called on a DPI or display
/// change, a hot-plug (`TIMER_DISPLAYS`) and when the choice changes.
/// `IslandWindow::reposition` calls `SetWindowPos`, which can send
/// `WM_DPICHANGED` straight back here; the window is taken out of `App` for
/// the call, so no borrow is held and the nested call finds it gone and returns.
/// The monitors are listed here, outside any borrow.
fn reposition() {
    let Some((taken, choice)) = with_app(|app| (app.win.take(), app.config.notch_monitor.clone()))
    else {
        // Arrived mid-handler: redo once the handler returns.
        DEFERRED_REPOSITION.set(true);
        return;
    };
    let Some(mut win) = taken else {
        // Nested inside reposition itself, which already reads the new placement.
        return;
    };
    let displays = arrange_win::displays();
    let target = notch_monitor::resolve(&choice, &displays).map(|i| displays[i].clone());
    win.reposition(target.as_ref());
    with_app(move |app| {
        app.win = Some(win);
        app.placed = target;
        app.displays = displays;
        // Another screen, DPI or position: all of it is drawn again.
        app.renderer.invalidate();
        // The screen's width caps the lyric notch; refresh draws.
        app.refresh(Instant::now());
        app.sync_settings();
        // The desktop notes follow their monitors.
        app.widgets_displays();
    });
}

/// `TIMER_DISPLAYS`: a device-node change settled. The monitors are listed
/// again and the island moves only when the screen it should be on is not
/// where it is (a different rectangle, DPI or device path). Outside
/// `with_app`: `reposition` needs the app unborrowed.
fn on_displays_timer(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_DISPLAYS);
    }
    let displays = arrange_win::displays();
    let Some((choice, placed)) =
        with_app(|app| (app.config.notch_monitor.clone(), app.placed.clone()))
    else {
        // A handler holds the app: ask again shortly.
        unsafe {
            SetTimer(Some(hwnd), TIMER_DISPLAYS, DISPLAYS_MS, None);
        }
        return;
    };
    let wanted = notch_monitor::resolve(&choice, &displays).map(|i| &displays[i]);
    if notch_monitor::moved(placed.as_ref(), wanted) {
        reposition();
    } else {
        with_app(move |app| {
            app.displays = displays;
            app.sync_settings();
            app.widgets_displays();
        });
    }
}

/// Runs work that `DEFERRED_*` postponed, once no handler holds the app.
fn run_deferred() {
    if APP.with(|cell| cell.try_borrow_mut().is_err()) {
        return;
    }
    if DEFERRED_REPOSITION.take() {
        reposition();
    }
    if DEFERRED_DRAIN.take() {
        with_app(App::drain);
    }
    if DEFERRED_FRAME.take() {
        with_app(App::animate);
    }
    for result in DEFERRED_RESULTS.take() {
        deliver_result(result);
    }
    if DEFERRED_SCREEN_PICK.take() {
        deliver_screen_pick();
    }
    if let Some(minimised) = DEFERRED_SCREEN_WATCH.take() {
        with_app(|app| app.screen_on_window(minimised));
    }
    if DEFERRED_SCREEN_ESC.take() {
        with_app(App::screen_cancel_countdown);
    }
    for action in DEFERRED_SETTINGS.take() {
        deliver_settings_action(action);
    }
    // A setting just applied (the Monitor choice) asks for a move: do it now,
    // not on the next message.
    if DEFERRED_REPOSITION.take() {
        reposition();
    }
    for key in DEFERRED_CLI.take() {
        deliver_cli_key(key);
    }
    for key in DEFERRED_CLIP.take() {
        deliver_clip_key(key);
    }
    for key in DEFERRED_NOTIF.take() {
        deliver_notif_key(key);
    }
    for key in DEFERRED_SEARCH.take() {
        deliver_search_key(key);
    }
    for (code, data) in DEFERRED_ARRANGE.take() {
        with_app(|app| app.on_arrange_msg(code, data));
    }
    if let Some(to) = DEFERRED_ISLAND_INACTIVE.take() {
        island_deactivated(to);
    }
    if DEFERRED_CLI_REDRAW.take() {
        with_app(App::on_input_changed);
    }
    if DEFERRED_OPEN_SETTINGS.take()
        && let Some(hwnd) = with_app(|app| app.hwnd)
    {
        open_settings(hwnd);
    }
    if let Some(card) = DEFERRED_OPEN_CARD.take()
        && let Some(hwnd) = with_app(|app| app.hwnd)
    {
        open_settings_card(hwnd, card);
    }
    if let Some((audio, rows, anchor)) = DEFERRED_TRANSCRIPT_MENU.take() {
        open_transcript_redo_menu(audio, rows, anchor);
    }
    if DEFERRED_SHUTDOWN.take()
        && let Some(hwnd) = with_app(|app| app.hwnd)
    {
        shutdown(hwnd);
    }
}

/// The island lost the activation to window `to`: the lifted notes hear of
/// it now, or once the handler holding the app returns.
fn island_deactivated(to: isize) {
    if with_app(|app| app.widgets_island_deactivated(HWND(to as _))).is_none() {
        DEFERRED_ISLAND_INACTIVE.set(Some(to));
    }
}

/// `deliver_cli_key` for the quick search's field.
fn deliver_search_key(key: InputKey) {
    let mut pending = Some(key);
    with_app(|app| {
        if let Some(key) = pending.take() {
            app.on_search_key(key);
        }
    });
    if let Some(key) = pending {
        DEFERRED_SEARCH.with_borrow_mut(|queue| queue.push(key));
    }
}

/// `deliver_cli_key` for the Notifikasi page's input.
fn deliver_notif_key(key: InputKey) {
    let mut pending = Some(key);
    with_app(|app| {
        if let Some(key) = pending.take() {
            app.on_notif_key(key);
        }
    });
    if let Some(key) = pending {
        DEFERRED_NOTIF.with_borrow_mut(|queue| queue.push(key));
    }
}

/// `deliver_cli_key` for the Clipboard search field.
fn deliver_clip_key(key: InputKey) {
    let mut pending = Some(key);
    with_app(|app| {
        if let Some(key) = pending.take() {
            app.on_clip_key(key);
        }
    });
    if let Some(key) = pending {
        DEFERRED_CLIP.with_borrow_mut(|queue| queue.push(key));
    }
}

/// Handles a key of the CLI input now, or once the handler holding the app
/// returns (the EDIT loses the keyboard inside `SetForegroundWindow`).
fn deliver_cli_key(key: InputKey) {
    let mut pending = Some(key);
    with_app(|app| {
        if let Some(key) = pending.take() {
            app.on_cli_key(key);
        }
    });
    if let Some(key) = pending {
        DEFERRED_CLI.with_borrow_mut(|queue| queue.push(key));
    }
}

/// `TIMER_SHORTCUT`: runs the waiting shortcut outside the app borrow (the
/// shell may pump messages). If the app is busy, the timer fires again.
fn run_shortcut() {
    if let Some(action) = with_app(App::take_shortcut).flatten() {
        // The capture overlay and the picker start here, a moment after they
        // were queued: a tip that began waiting in between must not cover them.
        crate::ui::tooltip::hide_all();
        shortcuts::run(action);
    }
}

/// `WM_SETTINGCHANGE` for "ImmersiveColorSet": the Windows app mode (light or
/// dark) or the accent changed.
fn is_color_set_change(lparam: LPARAM) -> bool {
    let area = PCWSTR(lparam.0 as *const u16);
    !area.is_null() && unsafe { area.to_string() }.is_ok_and(|a| a == "ImmersiveColorSet")
}

/// The panel that fades out while the island morphs into a compact view or
/// notice: what `closed_panel` kept, only if that panel was on screen when
/// the morph started (`from_visible`) and is the page that was drawn. A
/// panel that went off screen with a fullscreen app has nothing to fade when
/// the island comes back.
fn leaving_snapshot(
    was_panel: Option<Page>,
    from_visible: bool,
    closed: Option<ShownPanel>,
) -> Option<ShownPanel> {
    if !from_visible {
        return None;
    }
    closed.filter(|shown| Some(shown.page) == was_panel)
}

/// Where a morph that `refresh` starts is heading, for `leaving_on_morph`.
#[derive(Clone, Copy, Debug)]
struct MorphTo {
    /// To a panel.
    is_panel: bool,
    /// To Hidden (a fullscreen app): the shape stays and fades out.
    hiding: bool,
    /// To another compact view or notice than the one drawn.
    compact_swap: bool,
}

/// `closed_panel` after an applied event (`follow_closed_panel`): gone
/// while a panel shows, else the panel that showed before the event and
/// does not now (`shown_before`), else what it kept.
fn closed_panel_after(
    view: &View,
    shown_before: Option<ShownPanel>,
    closed: Option<ShownPanel>,
) -> Option<ShownPanel> {
    if matches!(view, View::Panel(_)) {
        None
    } else {
        shown_before.or(closed)
    }
}

/// `(leaving, closed_panel)` once a morph starts. A panel opening, or the
/// island going off screen (nothing of the panel is left to fade out when
/// it comes back), clears both. A panel closing into a compact view or a
/// notice hands its snapshot to the fade (`leaving_snapshot`). Any other
/// retarget (a close retargeted mid-way) keeps them: the fade goes on.
fn leaving_on_morph(
    to: MorphTo,
    was_panel: Option<Page>,
    from_visible: bool,
    leaving: Option<ShownPanel>,
    closed: Option<ShownPanel>,
) -> (Option<ShownPanel>, Option<ShownPanel>) {
    if to.is_panel || to.hiding {
        (None, None)
    } else if to.compact_swap && was_panel.is_some() {
        // As it showed before its close reset it; without that, no
        // fade-out rather than content that is not what was there.
        (leaving_snapshot(was_panel, from_visible, closed), None)
    } else {
        (leaving, closed)
    }
}

/// The height of the panel a snapshot showed, as `paint_leaving_panel`
/// sizes it.
fn panel_rest_h(style: Style, shown: &ShownPanel) -> f32 {
    shape_for(
        style,
        panel_kind(shown.page, shown.cli_tall, shown.cleaner.is_some()),
    )
    .h
}

/// Windows' "Animation effects" (Settings, Accessibility, Visual effects;
/// `SPI_GETCLIENTAREAANIMATION`). On when it cannot be read.
fn client_animations_on() -> bool {
    // A BOOL: four bytes.
    let mut on: i32 = 1;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut i32 as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || on != 0
}

/// Windows switched between light and dark apps: windows following it restyle.
/// The island itself is dark either way.
fn on_system_theme_changed() {
    // The tray menu follows Windows whatever Vale's window theme is.
    tray::refresh_menu_theme();
    let changed = theme::set_system_light(theme::read_system_light());
    if changed && theme::current().window == theme::WindowTheme::System {
        crate::ui::tooltip::hide_all();
        editor::restyle_open_editor();
        crate::shot_edit::window::restyle_open();
        settings::restyle();
        crate::transcript_win::restyle();
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = match msg {
        WM_APP_EVENT => {
            if with_app(App::drain).is_none() {
                DEFERRED_DRAIN.set(true);
            }
            LRESULT(0)
        }
        WM_APP_FRAME => {
            // The clock posts nothing more until this tick is handled, so it must not be lost.
            if with_app(App::animate).is_none() {
                DEFERRED_FRAME.set(true);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_SHORTCUT => {
            run_shortcut();
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_DISPLAYS => {
            on_displays_timer(hwnd);
            LRESULT(0)
        }
        // What the user asked a transcript window for.
        crate::transcript_win::WM_APP_TRANSCRIPT => {
            if with_app(App::on_transcript_requests).is_none() {
                // A handler holds the app: the requests are still queued; ask again.
                unsafe {
                    let _ = PostMessageW(
                        Some(hwnd),
                        crate::transcript_win::WM_APP_TRANSCRIPT,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            LRESULT(0)
        }
        // What the user did to a note on the desktop (`widgets::host`).
        WM_APP_WIDGET => {
            if with_app(App::widgets_events).is_none() {
                // A handler holds the app: the events are still queued; ask again.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_APP_WIDGET, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        // The tick that may draw nothing: it does not count as news.
        WM_TIMER if wparam.0 == TIMER_EQ => {
            with_app_quiet(|app| app.on_timer(TIMER_EQ));
            LRESULT(0)
        }
        WM_TIMER => {
            with_app(|app| app.on_timer(wparam.0));
            LRESULT(0)
        }
        WM_TIMECHANGE | WM_POWERBROADCAST => {
            if is_clock_jump(msg, wparam) {
                with_app(App::on_clock_jump);
            }
            if msg == WM_POWERBROADCAST {
                match wparam.0 {
                    // Back from sleep: the first screen of the day may greet.
                    PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC => {
                        let trigger = GreetTrigger::Wake {
                            by_user: wparam.0 == PBT_APMRESUMESUSPEND,
                        };
                        with_app(|app| {
                            // A recording the sleep paused goes on; a
                            // screen recording waits for the unlock.
                            app.rec_on_resume();
                            app.screen_on_away(app.session_locked());
                            // A phone session left over from before the
                            // sleep is ended: its connections are gone.
                            app.phone_on_resume(Instant::now());
                            app.maybe_greet(trigger)
                        });
                    }
                    // Going to sleep: the phone link ends with it, and a
                    // recording pauses (QPC runs on through the sleep).
                    PBT_APMSUSPEND => {
                        with_app(|app| {
                            app.rec_on_suspend();
                            app.screen_on_away(true);
                            app.phone_on_away(crate::phone::EndReason::Suspend, Instant::now())
                        });
                    }
                    // Charger or Energy Saver changed: the robot follows now.
                    PBT_APMPOWERSTATUSCHANGE => {
                        with_app(App::check_battery);
                    }
                    // The display went off or came back (dimmed counts as on).
                    w if w == PBT_POWERSETTINGCHANGE as usize => {
                        if let Some(off) = unsafe { display_off_from(lparam) } {
                            with_app(|app| {
                                app.display_off = off;
                                app.sync_pointer_polls();
                            });
                        }
                    }
                    _ => {}
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_WTSSESSION_CHANGE => {
            match wparam.0 {
                WTS_SESSION_LOCK => {
                    with_app(|app| {
                        app.session_locked = true;
                        // The lock screen is not recorded.
                        app.screen_on_away(true);
                        app.clip_on_lock();
                        app.phone_on_away(crate::phone::EndReason::Lock, Instant::now());
                        app.sync_pointer_polls();
                    });
                }
                WTS_SESSION_UNLOCK => {
                    with_app(|app| {
                        app.session_locked = false;
                        app.screen_on_away(false);
                        app.sync_pointer_polls();
                        app.maybe_greet(GreetTrigger::Unlock);
                    });
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(|app| app.on_mouse_move(lparam));
            // After the app is let go: the tooltip may make a window of its
            // own. A move while the app is held tells it nothing.
            if let Some(Some(tip)) = with_app(|app| app.tip_under(wparam, lparam)) {
                tooltip::hover(hwnd, tip);
            }
            // A card pulled off the shelf: the drag blocks until the button
            // is let go, so it starts outside the app borrow.
            if let Some(path) = with_app(|app| app.shelf_drag_start(wparam, lparam)).flatten() {
                shelf_ole::drag_out(&path, shelf_ole::SHELF_DRAG_EFFECTS);
                with_app(App::shelf_after_drag_out);
            }
            // An image card pulled off the Clipboard page, as its temp PNG.
            if let Some(out) = with_app(|app| app.clip_drag_start(wparam, lparam)).flatten() {
                let effect = shelf_ole::drag_out(&out.path, shelf_ole::CLIP_DRAG_EFFECTS);
                with_app(|app| app.clip_after_drag_out(&out, effect));
            }
            LRESULT(0)
        }
        WM_MOUSEHOVER => {
            with_app(|app| app.on_mouse_hover(lparam));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(App::on_mouse_leave);
            LRESULT(0)
        }
        WM_MBUTTONDOWN => {
            island_pressed(hwnd, lparam);
            with_app(|app| app.on_middle_press(lparam));
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            with_app(|app| app.on_middle_release(lparam));
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            island_pressed(hwnd, lparam);
            with_app(App::on_right_press);
            LRESULT(0)
        }
        // The side buttons do nothing on the island but still count as a press.
        WM_XBUTTONDOWN => {
            island_pressed(hwnd, lparam);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEHWHEEL => {
            let delta = (wparam.0 >> 16) as u16 as i16;
            with_app(|app| app.on_hwheel(i32::from(delta), lparam));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // High word of wParam: signed delta, +120 per notch away from the user.
            let delta = (wparam.0 >> 16) as u16 as i16;
            with_app(|app| app.on_wheel(i32::from(delta), lparam));
            LRESULT(0)
        }
        WM_COMMAND => {
            // The hidden CLI input's text changed (typing, paste, undo).
            if ((wparam.0 >> 16) & 0xFFFF) as u32 == EN_CHANGE {
                cli_input::request_redraw();
            }
            LRESULT(0)
        }
        arrange_overlay::WM_APP_ARRANGE => {
            let (code, data) = (wparam.0, lparam.0);
            if with_app(|app| app.on_arrange_msg(code, data)).is_none() {
                DEFERRED_ARRANGE.with_borrow_mut(|queue| queue.push((code, data)));
            }
            LRESULT(0)
        }
        WM_APP_CLI_REDRAW => {
            if with_app(App::on_input_changed).is_none() {
                DEFERRED_CLI_REDRAW.set(true);
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            with_app(App::on_clipboard_update);
            LRESULT(0)
        }
        WM_HOTKEY => {
            // A key press: any tip goes, whatever window it is on and
            // whatever the key does.
            tooltip::hide_all();
            let wm = wparam.0 as i32;
            if is_countdown_esc(wm) {
                // A press while the app is borrowed still cancels (the
                // recording must not start against the user's Esc).
                if with_app(App::screen_cancel_countdown).is_none() {
                    DEFERRED_SCREEN_ESC.set(true);
                }
            } else if let Some(id) = row_of_wm_id(wm) {
                with_app(|app| app.on_hotkey(id));
            }
            LRESULT(0)
        }
        // A PrtSc combo, seen by the keyboard hook thread (`app_prtsc`): the
        // row is in `wparam`.
        WM_APP_PRTSC => {
            let row = wparam.0;
            // A key press, as `WM_HOTKEY`.
            tooltip::hide_all();
            with_app(|app| app.on_hook_row(row));
            LRESULT(0)
        }
        // The floating lyrics card: what the user did in it (it never calls
        // `App` itself).
        crate::lyrics_card::WM_APP_LYRICS_CARD => {
            let (code, value) = (wparam.0, lparam.0);
            with_app(|app| app.on_lyrics_card_msg(code, value));
            LRESULT(0)
        }
        // The caption card, the same way.
        crate::caption_card::WM_APP_CAPTION_CARD => {
            let (code, value) = (wparam.0, lparam.0);
            with_app(|app| app.on_caption_card_msg(code, value));
            LRESULT(0)
        }
        // A drive came or went: only an arrival or a removal of a volume or
        // disk starts the pause before a scan. TRUE grants the request.
        WM_DEVICECHANGE => {
            // Not through `with_app`: a broadcast sent while `App` is
            // borrowed would be dropped, and the drive would go unseen.
            devices_page::on_device_change(hwnd, wparam.0, lparam.0);
            // A screen may have come or gone (Windows sends this in bursts):
            // look again once it settles. Also without `with_app`.
            if wparam.0 == DBT_DEVNODES_CHANGED as usize {
                unsafe {
                    SetTimer(Some(hwnd), TIMER_DISPLAYS, DISPLAYS_MS, None);
                }
            }
            LRESULT(1)
        }
        WM_ACTIVATE => {
            // The island lost activation (a click elsewhere, Alt+Tab): the
            // CLI input or the Clipboard search lets go.
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                deliver_cli_key(InputKey::Blur);
                deliver_clip_key(InputKey::Blur);
                deliver_notif_key(InputKey::Blur);
                deliver_search_key(InputKey::Blur);
                island_deactivated(lparam.0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_LBUTTONDOWN => {
            // The island never takes the activation, so the open dropdown
            // does not lose it to a press here: the press only closes it, as
            // a native menu's would. Also one Settings opened: it keeps the
            // activation while the island is pressed.
            island_pressed(hwnd, lparam);
            if crate::ui::menu_pop::is_open() {
                crate::ui::menu_pop::close();
                return LRESULT(0);
            }
            with_app(|app| app.on_press(lparam));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(|app| app.on_release(lparam));
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with_app(App::on_capture_lost);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            // A favourite in the open CLI folder menu unstars; elsewhere
            // the tray menu opens (outside the app borrow: it is modal).
            match with_app(|app| app.on_right_click(lparam)) {
                Some(RightClick::Tray) | None => open_menu(hwnd),
                Some(RightClick::NoteMenu(id)) => open_note_card_menu(hwnd, &id),
                Some(RightClick::RecRowMenu(i)) => open_rec_row_menu(hwnd, i),
                Some(RightClick::Done) => {}
            }
            LRESULT(0)
        }
        WM_APP_TRAY => {
            if tray::is_menu_click(lparam) {
                open_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_APP_ACCESS => {
            on_access_answered();
            LRESULT(0)
        }
        // The device dropdown went (picked or not).
        crate::ui::menu_pop::WM_APP_MENU_DONE => {
            if let Some(done) = unsafe { crate::ui::menu_pop::take_done(lparam) } {
                let pick = crate::ui::device_menu::picked(&done);
                let choice = crate::ui::transcript_menu::picked(&done);
                let quality = crate::ui::quality_menu::picked(&done);
                with_app(|app| {
                    app.rec_menu_done(pick);
                    if quality.is_some() {
                        app.rec_quality_done(quality);
                    }
                    if let Some(choice) = choice {
                        app.transcript_menu_choice(choice);
                    }
                });
            }
            LRESULT(0)
        }
        crate::capture::WM_APP_SCREEN_PICK => {
            deliver_screen_pick();
            LRESULT(0)
        }
        WM_APP_SCREEN_WATCH => {
            let minimised = wparam.0 != 0;
            if with_app(|app| app.screen_on_window(minimised)).is_none() {
                DEFERRED_SCREEN_WATCH.set(Some(minimised));
            }
            LRESULT(0)
        }
        crate::screenrec::wheel::WM_APP_ZOOM_WHEEL => {
            let steps = wparam.0 as isize;
            let dir = steps.signum() as i32;
            with_app(|app| {
                for _ in 0..steps.unsigned_abs() {
                    app.screen_zoom_step(dir);
                }
            });
            LRESULT(0)
        }
        WM_APP_REC_DEVICE => {
            let source = if wparam.0 == 0 {
                crate::recorder::Source::Mic
            } else {
                crate::recorder::Source::System
            };
            open_rec_device_menu(hwnd, source);
            LRESULT(0)
        }
        WM_APP_REC_QUALITY => {
            open_rec_quality_menu(hwnd);
            LRESULT(0)
        }
        WM_APP_SETTINGS_ACTION => {
            // Each action is taken exactly once, here.
            if let Some(action) = unsafe { settings::take_action(lparam) } {
                deliver_settings_action(action);
            }
            LRESULT(0)
        }
        WM_APP_EDITOR_RESULT => {
            // Each result is taken exactly once, here.
            if let Some(result) = unsafe { editor::take_result(lparam) } {
                deliver_result(result);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            shutdown(hwnd);
            LRESULT(0)
        }
        // Logoff or shutdown: the process may end right after this returns.
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                flush_editor();
                // A history change still waiting for its save is written now.
                with_app(App::clip_shutdown);
                with_app(App::notif_shutdown);
                with_app(App::widgets_flush);
                with_app(App::hotkeys_shutdown);
                with_app(App::screen_shutdown);
                with_app(App::rec_shutdown);
                with_app(App::transcribe_shutdown);
                with_app(App::captions_shutdown);
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            reposition();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            if is_color_set_change(lparam) {
                on_system_theme_changed();
            }
            // The animation switch is read again (no timer, no poll). It
            // matters to the next transition, the shrunk eyes and the dancing
            // cat (`TIMER_EQ`), so a change re-syncs them and redraws.
            with_app(|app| app.animations_changed());
            // The desktop notes draw once with Windows' colours as they are now.
            with_app(App::widgets_restyle);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_DESTROY => {
            drain_editor_results(hwnd);
            with_app(App::stop_sync);
            with_app(App::clip_shutdown);
            with_app(App::notif_shutdown);
            with_app(App::widgets_shutdown);
            with_app(App::devices_shutdown);
            with_app(App::arrange_shutdown);
            with_app(App::hotkeys_shutdown);
            with_app(App::screen_shutdown);
            with_app(App::rec_shutdown);
            with_app(App::transcribe_shutdown);
            with_app(App::captions_shutdown);
            unsafe {
                let _ = WTSUnRegisterSessionNotification(hwnd);
                let note = DISPLAY_NOTIFY.replace(0);
                if note != 0 {
                    let _ = UnregisterPowerSettingNotification(HPOWERNOTIFY(note));
                }
            }
            tray::remove_tray(hwnd);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if tray::is_taskbar_created(msg) => {
            let _ = tray::add_tray(hwnd);
            // The notes on the desktop layer are owned by the shell's window.
            with_app(App::widgets_taskbar);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    };
    run_deferred();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::geometry::{LYRIC_RESIZE_MS, lyric_shape, lyric_width};

    #[test]
    fn eq_key_keeps_progress_for_the_compact_view_only() {
        let p = Some(Progress {
            position_ms: 1_000,
            end_ms: 200_000,
        });
        assert_eq!(key_progress(true, p), p);
        assert_eq!(key_progress(false, p), None);
    }

    fn progress_view(pct: u8) -> View {
        View::Notice(Notice {
            key: "update-progress".into(),
            source: "Vale".into(),
            title: format!("{pct}%"),
            body: String::new(),
            kind: NoticeKind::UpdateProgress { pct },
        })
    }

    #[test]
    fn a_new_percent_is_the_same_view_but_any_other_change_is_not() {
        assert!(progress_in_place(&progress_view(1), &progress_view(2)));
        assert!(progress_in_place(&progress_view(1), &progress_view(1)));
        assert!(!progress_in_place(&progress_view(1), &View::Idle));
        assert!(!progress_in_place(&View::Idle, &progress_view(1)));
        assert!(!progress_in_place(&View::Hidden, &View::Hidden));
    }

    /// A mutex name of this test's own, so no real island is involved.
    fn mutex_name(tag: &str) -> windows::core::HSTRING {
        windows::core::HSTRING::from(format!("Local\\ValeTest-{tag}-{}", std::process::id()))
    }

    #[test]
    fn a_taken_mutex_quits_at_once_and_a_waiting_start_gets_it_when_the_owner_ends() {
        let name = mutex_name("claim");
        let names = [PCWSTR(name.as_ptr())];
        // The "old island": holds the mutex through one handle.
        let owner = unsafe { CreateMutexW(None, false, names[0]) }.expect("owner mutex");
        // A normal start quits at once.
        let start = Instant::now();
        assert!(!claim_names(&names, Duration::ZERO));
        assert!(start.elapsed() < Duration::from_millis(150));
        // The owner is still alive, so a short wait ends without it.
        assert!(!claim_names(&names, Duration::from_millis(450)));
        // The owner ends (its handle closes) while a start waits: the wait
        // gets the mutex. It would not if the failed tries had kept their own
        // handles, which keep the mutex alive after the owner is gone.
        let raw = owner.0 as isize;
        let closer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            unsafe {
                let _ = CloseHandle(windows::Win32::Foundation::HANDLE(raw as *mut c_void));
            }
        });
        let start = Instant::now();
        assert!(claim_names(&names, Duration::from_secs(5)));
        assert!(start.elapsed() < Duration::from_secs(4), "{:?}", start.elapsed());
        closer.join().unwrap();
    }

    #[test]
    fn the_failed_tries_leave_nothing_behind() {
        // Two names, one of them held by someone else: a failed try must also
        // release the name that was free, or the next try finds its own mutex.
        let held = mutex_name("held");
        let free = mutex_name("free");
        let (held_p, free_p) = (PCWSTR(held.as_ptr()), PCWSTR(free.as_ptr()));
        // Held by "someone else": a handle this test owns and closes later.
        let owner = unsafe { CreateMutexW(None, false, held_p) }.expect("owner mutex");
        for _ in 0..3 {
            assert!(!try_claim(&[free_p, held_p]), "held name taken");
        }
        // The owner ends: both names are free at once.
        unsafe {
            let _ = CloseHandle(owner);
        }
        assert!(try_claim(&[free_p, held_p]));
        // Held until `release_single_instance`; claimed again it is taken.
        assert!(!try_claim(&[free_p]));
    }

    #[test]
    fn only_a_start_after_an_update_waits_for_the_old_island() {
        assert_eq!(claim_wait(&Start::Normal), Duration::ZERO);
        assert_eq!(
            claim_wait(&Start::Updated("3.15.0".into())),
            Duration::from_secs(60)
        );
        // Even an empty old version (the updater passed none) is an update.
        assert_eq!(claim_wait(&Start::Updated(String::new())), UPDATE_CLAIM_WAIT);
        // Longer than Windows Error Reporting keeps a crashing process (~12 s).
        assert!(UPDATE_CLAIM_WAIT >= Duration::from_secs(60));
    }

    #[test]
    fn claim_wait_waits_after_an_import() {
        assert_eq!(claim_wait(&Start::Imported), UPDATE_CLAIM_WAIT);
        assert_eq!(claim_wait(&Start::Updated("3.15.0".into())), UPDATE_CLAIM_WAIT);
        assert_eq!(claim_wait(&Start::Normal), Duration::ZERO);
    }

    #[test]
    fn releasing_the_mutexes_lets_the_next_island_claim_them() {
        let first = mutex_name("release-a");
        let second = mutex_name("release-b");
        let names = [PCWSTR(first.as_ptr()), PCWSTR(second.as_ptr())];
        assert!(claim_names(&names, Duration::ZERO), "unique names are free");
        // The island holds both: a second start quits at once.
        assert!(!claim_names(&names, Duration::ZERO));
        assert!(!try_claim(&names[..1]));
        assert!(!try_claim(&names[1..]));
        // Its teardown hands them over: both names are free again.
        release_single_instance();
        assert!(claim_names(&names, Duration::ZERO));
        // And once more, to leave nothing held by this test thread.
        release_single_instance();
        assert!(try_claim(&names));
        release_single_instance();
    }

    #[test]
    fn the_own_pick_overlay_is_never_taken_for_a_fullscreen_app() {
        // Overlay up: nothing is learned, either way.
        assert_eq!(fullscreen_event(true, true, false), None);
        assert_eq!(fullscreen_event(false, true, true), None);
        assert_eq!(fullscreen_event(true, true, true), None);
        assert_eq!(fullscreen_event(false, true, false), None);
        // Overlay down: a change is reported, the same value is not.
        assert_eq!(fullscreen_event(true, false, false), Some(true));
        assert_eq!(fullscreen_event(false, false, true), Some(false));
        assert_eq!(fullscreen_event(true, false, true), None);
        assert_eq!(fullscreen_event(false, false, false), None);
    }

    #[test]
    fn greeting_waits_for_someone_at_an_unlocked_screen() {
        use GreetTrigger::*;
        // Windows locks on wake by default: the wake keeps the day, the
        // unlock that follows greets.
        assert!(!greet_may_try(Wake { by_user: true }, true));
        assert!(
            greet_may_try(Unlock, true),
            "the unlock message itself counts"
        );
        assert!(greet_may_try(Unlock, false));
        // No lock on wake: the user's wake greets at once.
        assert!(greet_may_try(Wake { by_user: true }, false));
        // An unattended wake (maintenance, Modern Standby) never greets.
        assert!(!greet_may_try(Wake { by_user: false }, false));
        assert!(!greet_may_try(Wake { by_user: false }, true));
        assert!(greet_may_try(Startup, false));
        assert!(!greet_may_try(Startup, true));
    }

    fn shown(page: Page) -> ShownPanel {
        ShownPanel {
            page,
            cleaner: None,
            mole: crate::state::MolePhase::Unknown,
            cli_tall: false,
            cli_menu: false,
            notes_page: 0,
            reminders_page: 0,
            day_page: 0,
            strip: None,
        }
    }

    #[test]
    fn a_visible_panel_closing_hands_its_snapshot_to_the_fade() {
        let snapshot = shown(Page::Notes);
        assert_eq!(
            leaving_snapshot(Some(Page::Notes), true, Some(snapshot.clone())),
            Some(snapshot.clone())
        );
        // Not the page that was drawn: no fade-out of the wrong content.
        assert_eq!(
            leaving_snapshot(Some(Page::Media), true, Some(snapshot)),
            None
        );
        assert_eq!(leaving_snapshot(Some(Page::Notes), true, None), None);
    }

    fn morph_to(is_panel: bool, hiding: bool, compact_swap: bool) -> MorphTo {
        MorphTo {
            is_panel,
            hiding,
            compact_swap,
        }
    }

    #[test]
    fn a_fullscreen_hide_then_an_idle_return_fades_no_panel() {
        let notes = Some(Page::Notes);
        // The Notes panel shows: nothing is kept.
        let closed = closed_panel_after(&View::Panel(Page::Notes), None, None);
        assert_eq!(closed, None);
        // Fullscreen(true): the view is Hidden, and the event keeps the
        // panel as it showed (`follow_closed_panel`).
        let closed = closed_panel_after(&View::Hidden, Some(shown(Page::Notes)), closed);
        assert_eq!(closed, Some(shown(Page::Notes)));
        // `refresh` retargets to Hidden (`drawn` still says Panel, the
        // panel was visible): both are cleared.
        let (leaving, closed) =
            leaving_on_morph(morph_to(false, true, false), notes, true, None, closed);
        assert_eq!((&leaving, &closed), (&None, &None));
        // The game ends: the event that brings Idle back had no panel
        // before it, so nothing is stored again.
        let closed = closed_panel_after(&View::Idle, None, closed);
        assert_eq!(closed, None);
        // `refresh` to Idle: `drawn` still says Panel, so it is a compact
        // swap from a panel, but the island was hidden: no fade.
        let from_visible = false;
        let (leaving, closed) = leaving_on_morph(
            morph_to(false, false, true),
            notes,
            from_visible,
            leaving,
            closed,
        );
        assert_eq!((leaving, closed), (None, None));
        // Even a snapshot that survived is not handed over from hidden.
        let (leaving, _) = leaving_on_morph(
            morph_to(false, false, true),
            notes,
            from_visible,
            None,
            Some(shown(Page::Notes)),
        );
        assert_eq!(leaving, None);
    }

    #[test]
    fn the_leaving_snapshot_follows_the_morph() {
        let notes = Some(Page::Notes);
        let snapshot = Some(shown(Page::Notes));
        // A visible panel closing into a compact view: the snapshot fades.
        let (leaving, closed) = leaving_on_morph(
            morph_to(false, false, true),
            notes,
            true,
            None,
            snapshot.clone(),
        );
        assert_eq!((&leaving, &closed), (&snapshot, &None));
        // Retargeted mid-close (no panel drawn now): the fade goes on.
        let (kept, closed) = leaving_on_morph(
            morph_to(false, false, true),
            None,
            true,
            leaving.clone(),
            closed,
        );
        assert_eq!((&kept, &closed), (&snapshot, &None));
        let (kept, _) = leaving_on_morph(morph_to(false, false, false), None, true, leaving, None);
        assert_eq!(kept, snapshot);
        // Reopened mid-close: the panel shows itself, nothing leaves.
        let (leaving, closed) =
            leaving_on_morph(morph_to(true, false, false), None, true, kept, snapshot);
        assert_eq!((leaving, closed), (None, None));
    }

    #[test]
    fn the_closing_panel_is_sized_as_it_showed() {
        let tall = ShownPanel {
            cli_tall: true,
            ..shown(Page::Cli)
        };
        assert_eq!(
            panel_rest_h(Style::Notch, &tall),
            shape_for(Style::Notch, ShapeKind::PanelTall).h
        );
        assert_eq!(
            panel_rest_h(Style::Notch, &shown(Page::Notes)),
            shape_for(Style::Notch, ShapeKind::Panel).h
        );
    }

    #[test]
    fn session_lock_query_runs() {
        // Desktop or CI service session: whatever it answers, reading and
        // freeing Windows' buffer must not crash.
        let _ = query_session_locked();
    }

    #[test]
    fn a_rising_island_is_drawn_dimmer_and_higher() {
        let t0 = Instant::now();
        let shape = shape_for(Style::Pill, ShapeKind::Idle);
        let rise = behind::Rise::new(t0, true);
        // At its start: invisible, 8 dip above its place; the rest is kept.
        let (high, opacity) = rise_frame(shape, 1.0, Some(&rise), t0);
        assert_eq!(opacity, 0.0);
        assert_eq!(high.y, shape.y - behind::RISE_DROP_DIP);
        assert_eq!(
            (high.x, high.w, high.h, high.radius),
            (shape.x, shape.w, shape.h, shape.radius)
        );
        // A morph's own opacity is multiplied, not replaced.
        let later = t0 + Duration::from_millis(90);
        let factor = rise.at(later).0;
        let (_, opacity) = rise_frame(shape, 0.5, Some(&rise), later);
        assert_eq!(opacity, 0.5 * factor);
        // No rise, or a finished one: the input unchanged.
        assert_eq!(rise_frame(shape, 0.5, None, t0), (shape, 0.5));
        let done = t0 + Duration::from_millis(behind::RISE_MS);
        assert_eq!(rise_frame(shape, 0.5, Some(&rise), done), (shape, 0.5));
    }

    fn morph(start: Instant) -> Anim {
        Anim {
            from: shape_for(Style::Pill, ShapeKind::Idle),
            from_opacity: 0.0,
            to: shape_for(Style::Pill, ShapeKind::Notice),
            to_opacity: 1.0,
            start,
            ms: ANIM_MS,
            running: true,
            curve: MorphCurve::Spring,
            fade_from: None,
        }
    }

    #[test]
    fn an_eyes_morph_takes_320_ms_and_never_overshoots() {
        let t0 = Instant::now();
        let (ms, curve) = morph_timing(true, false, false);
        let eyes = Anim {
            ms,
            curve,
            fade_from: Some(0.0),
            ..morph(t0)
        };
        let target = eyes.to;
        let at = |n: u64| t0 + Duration::from_millis(n);
        assert_ne!(
            eyes.frame(at(ANIM_MS)).0,
            target,
            "not there yet at the view morph's 240 ms"
        );
        assert!(eyes.fading(at(ANIM_MS)).is_some());
        assert_eq!(eyes.frame(at(320)).0, target);
        assert_eq!(eyes.fading(at(320)), None);
        for n in (0..=320).step_by(8) {
            let (shape, _) = eyes.frame(at(n));
            assert!(
                shape.w <= target.w + 1e-3 && shape.h <= target.h + 1e-3,
                "{n} ms"
            );
        }
        // The content fade follows the eased progress, not the linear one.
        let linear = Anim {
            curve: MorphCurve::Spring,
            ..eyes
        };
        assert!(eyes.fading(at(150)).unwrap() < linear.fading(at(150)).unwrap());
    }

    #[test]
    fn the_slot_keeps_its_share_while_a_longer_lyric_line_grows_the_notch() {
        // The slot shows before and after; the line side grows by 40 dip
        // (a new, longer line within the toast's 5 s). The logo must stay
        // whole the whole morph, overshoot included, not blink out and fade
        // back as it would if the share were read off the target width.
        let t0 = Instant::now();
        let base = lyric_shape(Style::Pill, 300.0);
        let wide = lyric_shape(Style::Pill, 340.0);
        let (ms, curve) = morph_timing(false, true, false);
        let anim = Anim {
            from: base,
            from_opacity: 1.0,
            to: wide,
            to_opacity: 1.0,
            start: t0,
            ms,
            running: true,
            curve,
            fade_from: None,
        };
        let both = SlotShare {
            from: TOAST_SLOT_EXTRA,
            to: TOAST_SLOT_EXTRA,
        };
        let none = SlotShare::default();
        for n in (0..=ms).step_by(4) {
            let at = t0 + Duration::from_millis(n);
            assert_eq!(both.at(&anim, at), TOAST_SLOT_EXTRA, "{n} ms");
            assert_eq!(none.at(&anim, at), 0.0, "{n} ms");
        }
        // A slot that comes or goes eases with the shape, clamped to its
        // room, and lands exactly on the target's share.
        let comes = SlotShare {
            from: 0.0,
            to: TOAST_SLOT_EXTRA,
        };
        let goes = SlotShare {
            from: TOAST_SLOT_EXTRA,
            to: 0.0,
        };
        let mut last = 0.0;
        for n in (0..=ms).step_by(4) {
            let at = t0 + Duration::from_millis(n);
            let e = comes.at(&anim, at);
            assert!((0.0..=TOAST_SLOT_EXTRA).contains(&e), "{n} ms: {e}");
            assert!(
                e >= last || e == TOAST_SLOT_EXTRA,
                "{n} ms: {e} after {last}"
            );
            last = e;
            assert!((0.0..=TOAST_SLOT_EXTRA).contains(&goes.at(&anim, at)));
        }
        assert_eq!(comes.at(&anim, t0), 0.0);
        assert_eq!(goes.at(&anim, t0), TOAST_SLOT_EXTRA);
        let done = t0 + Duration::from_millis(ms);
        assert_eq!(comes.at(&anim, done), TOAST_SLOT_EXTRA);
        assert_eq!(goes.at(&anim, done), 0.0);
        // Settled (no morph): the target's share.
        let settled = Anim::settled(wide, 1.0);
        assert_eq!(comes.at(&settled, t0), TOAST_SLOT_EXTRA);
        assert_eq!(goes.at(&settled, t0), 0.0);
    }

    #[test]
    fn a_fade_cut_short_reports_where_it_is() {
        let t0 = Instant::now();
        let mut anim = morph(t0);
        assert_eq!(anim.fading(t0), None, "no fade, nothing to carry on");
        anim.fade_from = Some(0.0);
        let mid = t0 + Duration::from_millis(ANIM_MS * 3 / 4);
        let a = anim.fading(mid).expect("still fading");
        assert!(a > 0.0 && a < 1.0);
        assert_eq!(anim.fading(t0 + Duration::from_millis(ANIM_MS)), None);
        anim.running = false;
        assert_eq!(anim.fading(mid), None);
    }

    #[test]
    fn frame_log_appends_intervals_per_animation() {
        let path = std::env::temp_dir().join(format!("di-frames-test-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut log = FrameLog {
            path: Some(path.clone()),
            last: None,
            intervals: Vec::new(),
        };
        let t0 = Instant::now();
        // A frame outside any animation is not logged.
        log.frame(t0);
        log.start(t0);
        log.frame(t0 + Duration::from_millis(10));
        log.frame(t0 + Duration::from_millis(26));
        // A retarget ends the running animation's entry.
        log.start(t0 + Duration::from_millis(30));
        log.frame(t0 + Duration::from_millis(37));
        log.finish();
        log.finish();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            text, "# animation\n10.00\n16.00\n# animation\n7.00\n",
            "{text:?}"
        );
    }

    #[test]
    fn frame_log_off_records_nothing() {
        let mut log = FrameLog {
            path: None,
            last: None,
            intervals: Vec::new(),
        };
        let t0 = Instant::now();
        log.start(t0);
        log.frame(t0 + Duration::from_millis(10));
        assert!(log.last.is_none() && log.intervals.is_empty());
    }

    #[test]
    fn island_notices_are_temporary_and_open_nothing() {
        for notice in [dnd_notice(), invalid_config_notice(None)] {
            assert_eq!(notice.source, "Vale");
            assert_eq!(
                notice.kind,
                NoticeKind::App {
                    app_id: None,
                    toast: None
                }
            );
        }
    }

    #[test]
    fn dnd_notice_text() {
        let n = dnd_notice();
        assert_eq!(n.title, "Matiin popup Windows");
        assert_eq!(
            n.body,
            "Nyalain Jangan Ganggu: Pengaturan → Sistem → Pemberitahuan"
        );
    }

    #[test]
    fn hover_needs_a_dwell_not_just_a_move() {
        let mut h = HoverTracking::default();
        // Entering arms tracking once; further moves do not re-arm or expand.
        assert!(h.needs_arming());
        h.armed();
        assert!(!h.needs_arming());
        // Leaving without a dwell never sent Hover(true), so nothing to undo.
        assert_eq!(h.on_leave(), None);
        assert!(h.needs_arming());
    }

    #[test]
    fn dwell_expands_once_and_leave_collapses() {
        let mut h = HoverTracking::default();
        h.armed();
        assert_eq!(h.on_hover(), Some(Event::Hover(true)));
        assert_eq!(h.on_hover(), None);
        assert!(!h.needs_arming());
        assert_eq!(h.on_leave(), Some(Event::Hover(false)));
        // Tracking ends on leave; the next move re-arms it.
        assert!(h.needs_arming());
    }

    #[test]
    fn a_notice_close_before_the_dwell_keeps_the_visit_from_hovering() {
        let mut h = HoverTracking::default();
        h.armed();
        // The ✕ was clicked before WM_MOUSEHOVER fired.
        h.close_quiet();
        assert_eq!(h.on_hover(), None);
        assert_eq!(h.on_hover(), None);
        // Hover(true) was never sent, so there is no Hover(false) either, and
        // the quiet flag ends with the visit.
        assert_eq!(h.on_leave(), None);
        assert!(h.needs_arming());
        h.armed();
        assert_eq!(h.on_hover(), Some(Event::Hover(true)));
        assert_eq!(h.on_leave(), Some(Event::Hover(false)));
    }

    #[test]
    fn a_notice_close_after_the_dwell_still_ends_the_visit_with_hover_false() {
        let mut h = HoverTracking::default();
        h.armed();
        assert_eq!(h.on_hover(), Some(Event::Hover(true)));
        h.close_quiet();
        assert_eq!(h.on_hover(), None);
        assert_eq!(h.on_leave(), Some(Event::Hover(false)));
        h.armed();
        assert_eq!(h.on_hover(), Some(Event::Hover(true)));
    }

    #[test]
    fn a_release_without_a_press_is_no_click() {
        // The button went down in another window and came up over the island.
        let mut c = ClickPair::default();
        assert_eq!(c.release(Some(Hit::PlayPause)), None);
    }

    #[test]
    fn press_and_release_must_be_on_the_same_target() {
        let mut c = ClickPair::default();
        c.press(Some(Hit::Prev));
        assert_eq!(c.release(Some(Hit::PlayPause)), None);
        // That release ended the press: a second release is no click either.
        assert_eq!(c.release(Some(Hit::Prev)), None);
        c.press(Some(Hit::PlayPause));
        assert_eq!(c.release(Some(Hit::PlayPause)), Some(Hit::PlayPause));
        assert_eq!(c.release(Some(Hit::PlayPause)), None, "one click per press");
        // Pressed over nothing, or let go over nothing.
        c.press(None);
        assert_eq!(c.release(Some(Hit::Body)), None);
        c.press(Some(Hit::Body));
        assert_eq!(c.release(None), None);
    }

    #[test]
    fn a_press_on_the_eyes_held_past_the_dwell_opens_nothing() {
        // The pointer rests on the eyes ~450 ms, presses, holds past 500 ms
        // and lets go: the dwell restored the notch under the held button.
        let (mut peek, t0) = (Peek::default(), Instant::now());
        let on_pill = PeekInputs {
            eligible: true,
            in_zone: true,
            over_pill: true,
            window_under: true,
            ..PeekInputs::default()
        };
        peek.step(
            PeekInputs {
                over_pill: false,
                ..on_pill
            },
            t0,
        );
        assert!(peek.is_shrunk());
        peek.step(on_pill, t0);
        assert!(peek.step(on_pill, t0 + Duration::from_millis(450)) != PeekState::Restored);
        let mut c = ClickPair::default();
        // Pressed on the eyes (their pill hits as the body).
        c.press(Some(Hit::Body));
        let was = peek.is_shrunk();
        assert_eq!(
            peek.step(on_pill, t0 + Duration::from_millis(500)),
            PeekState::Restored
        );
        let eyes_swap = was != peek.is_shrunk();
        // Same view before and after: only the swap ends the press.
        if press_ends(&View::Idle, &View::Idle, eyes_swap) {
            c.clear();
        }
        // The release on the restored notch, same spot: no click, no panel.
        assert_eq!(c.release(Some(Hit::Body)), None);
    }

    #[test]
    fn a_press_ends_with_the_view_or_the_eyes_swap() {
        assert!(!press_ends(&View::Idle, &View::Idle, false));
        assert!(press_ends(&View::Idle, &View::Idle, true));
        assert!(press_ends(&View::MediaCompact, &View::Idle, false));
        assert!(press_ends(&View::Hidden, &View::Hidden, false));
    }

    #[test]
    fn the_lyric_moves_only_while_its_view_shows_unshrunk() {
        assert!(lyric_moves(&View::MediaLyric, false));
        // Shrunk to the eyes: no crossfade, no scroll, no frame clock.
        assert!(!lyric_moves(&View::MediaLyric, true));
        assert!(!lyric_moves(&View::MediaCompact, false));
        assert!(!lyric_moves(&View::Idle, false));
    }

    #[test]
    fn leaving_ends_a_press() {
        let mut c = ClickPair::default();
        c.press(Some(Hit::Next));
        c.clear();
        assert_eq!(c.release(Some(Hit::Next)), None);
        // Targets with data compare whole.
        c.press(Some(Hit::NoteOpen("a".into())));
        assert_eq!(c.release(Some(Hit::NoteOpen("b".into()))), None);
        c.press(Some(Hit::NoteOpen("a".into())));
        assert_eq!(
            c.release(Some(Hit::NoteOpen("a".into()))),
            Some(Hit::NoteOpen("a".into()))
        );
    }

    #[test]
    fn settled_anim_rests_on_its_shape() {
        let shape = shape_for(Style::Notch, ShapeKind::Panel);
        let anim = Anim::settled(shape, 0.0);
        assert_eq!(anim.progress(Instant::now()), 1.0);
        assert_eq!(anim.frame(Instant::now()), (shape, 0.0));
    }

    #[test]
    fn anim_starts_at_from() {
        let t0 = Instant::now();
        let anim = morph(t0);
        assert_eq!(anim.frame(t0), (anim.from, 0.0));
    }

    #[test]
    fn anim_ends_exactly_on_target() {
        let t0 = Instant::now();
        let anim = morph(t0);
        let end = t0 + Duration::from_millis(ANIM_MS);
        assert_eq!(anim.progress(end), 1.0);
        assert_eq!(anim.frame(end), (anim.to, 1.0));
        assert_eq!(anim.frame(end + Duration::from_secs(5)), (anim.to, 1.0));
    }

    #[test]
    fn a_lyric_resize_morphs_quicker_with_the_same_spring() {
        let t0 = Instant::now();
        let resize = Anim {
            from: lyric_shape(Style::Notch, 300.0),
            to: lyric_shape(Style::Notch, 420.0),
            from_opacity: 1.0,
            ms: LYRIC_RESIZE_MS,
            ..morph(t0)
        };
        let half = t0 + Duration::from_millis(LYRIC_RESIZE_MS / 2);
        assert!((resize.progress(half) - 0.5).abs() < 0.01);
        let at = |anim: &Anim, ms| anim.progress(t0 + Duration::from_millis(ms));
        assert_eq!(at(&resize, LYRIC_RESIZE_MS), 1.0);
        assert!(
            at(&morph(t0), LYRIC_RESIZE_MS) < 1.0,
            "views still take ANIM_MS"
        );
        assert_eq!(at(&morph(t0), ANIM_MS), 1.0);
        // The same eased overshoot, only quicker.
        let (mid, _) = resize.frame(half);
        let eased = lerp_shape(
            resize.from,
            resize.to,
            crate::ui::geometry::ease_out_back(resize.progress(half)),
        );
        assert_eq!(mid, eased);
        assert_eq!(
            resize.frame(t0 + Duration::from_millis(LYRIC_RESIZE_MS)).0,
            resize.to
        );
    }

    #[test]
    fn lyric_fit_keeps_its_width_for_a_slightly_shorter_line() {
        let t0 = Instant::now();
        let mut fit = LyricFit::default();
        assert_eq!(fit.width(), LYRIC_MIN_W);
        fit.line(Some(300.0), t0, false, LYRIC_MAX_W);
        let first = fit.width();
        assert_eq!(first, lyric_width(Some(300.0), false, LYRIC_MAX_W));
        // 30 px shorter: the same width, so refresh does not retarget.
        fit.line(Some(270.0), t0, false, LYRIC_MAX_W);
        assert_eq!(fit.width(), first);
        assert_eq!(lyric_shape(Style::Pill, fit.width()).w, first);
        // 40 px or more shorter shrinks; longer grows at once.
        fit.line(Some(255.0), t0, false, LYRIC_MAX_W);
        assert_eq!(fit.width(), lyric_width(Some(255.0), false, LYRIC_MAX_W));
        fit.line(Some(400.0), t0, false, LYRIC_MAX_W);
        assert_eq!(fit.width(), lyric_width(Some(400.0), false, LYRIC_MAX_W));
        // The rest sign after a long line: kept wide only within the slack.
        fit.line(None, t0, false, LYRIC_MAX_W);
        assert_eq!(fit.width(), LYRIC_MIN_W);
        // Leaving the view starts the next one fresh.
        fit = LyricFit::default();
        fit.line(Some(100.0), t0, false, LYRIC_MAX_W);
        assert_eq!(fit.width(), LYRIC_MIN_W);
    }

    #[test]
    fn lyric_fit_floors_at_the_users_notch_width() {
        let t0 = Instant::now();
        let mut fit = LyricFit::default();
        // Wider than the lyric minimum: the empty notch takes it.
        fit.set_min(300.0);
        assert_eq!(fit.width(), 300.0);
        fit.line(None, t0, false, 300.0_f32.max(LYRIC_MAX_W));
        assert_eq!(fit.width(), 300.0);
        // A short line stays at the floor; a long one grows past it.
        // With the robot the notch is the floor plus the robot's extra lead,
        // so the line's box keeps the room it had.
        fit.line(Some(100.0), t0, true, LYRIC_MAX_W);
        assert_eq!(fit.width(), 300.0 + crate::ui::geometry::LYRIC_ROBOT_EXTRA);
        fit.line(Some(450.0), t0, true, LYRIC_MAX_W);
        assert!(fit.width() > 450.0 && fit.width() <= LYRIC_MAX_W);
        // Changing the width refits the same line; a narrower setting goes
        // back to the lyric minimum.
        fit.line(Some(100.0), t0, true, LYRIC_MAX_W);
        fit.set_min(160.0);
        assert_eq!(
            fit.width(),
            LYRIC_MIN_W + crate::ui::geometry::LYRIC_ROBOT_EXTRA
        );
        fit.set_min(260.0);
        assert_eq!(fit.width(), 260.0 + crate::ui::geometry::LYRIC_ROBOT_EXTRA);
        // The cap never sits under the floor.
        let mut tight = LyricFit::default();
        tight.set_min(300.0);
        tight.line(Some(400.0), t0, false, 250.0);
        assert_eq!(tight.width(), 250.0_f32.max(300.0));
    }

    #[test]
    fn lyric_fit_follows_the_robot_and_the_screen() {
        let t0 = Instant::now();
        let mut fit = LyricFit::default();
        fit.line(Some(300.0), t0, false, LYRIC_MAX_W);
        fit.refit(true, LYRIC_MAX_W);
        assert_eq!(fit.width(), lyric_width(Some(300.0), true, LYRIC_MAX_W));
        // A smaller screen caps the same line lower.
        fit.line(Some(900.0), t0, true, LYRIC_MAX_W);
        assert_eq!(
            fit.width(),
            LYRIC_MAX_W + crate::ui::geometry::LYRIC_ROBOT_EXTRA
        );
        fit.refit(true, 450.0);
        assert_eq!(fit.width(), 450.0 + crate::ui::geometry::LYRIC_ROBOT_EXTRA);
        // Nothing to refit before the first line.
        let mut empty = LyricFit::default();
        empty.refit(true, 450.0);
        assert_eq!(empty, LyricFit::default());
    }

    #[test]
    fn lyric_fit_with_a_toast_slot_keeps_the_line_box_and_never_passes_the_cap() {
        let t0 = Instant::now();
        for robot in [false, true] {
            // A short line: the slot only adds its room to the notch.
            let mut fit = LyricFit::default();
            fit.set_min(190.0);
            fit.line(Some(60.0), t0, robot, LYRIC_MAX_W);
            let (plain_w, plain_box) = (fit.width(), fit.box_w());
            fit.set_slot(true);
            assert_eq!(fit.width(), plain_w + TOAST_SLOT_EXTRA, "robot {robot}");
            assert_eq!(fit.box_w(), plain_box, "the line keeps its box");
            assert_eq!(fit.content_w(), plain_w);
            fit.set_slot(false);
            assert_eq!(fit.width(), plain_w);

            // A line at the cap: the total stays at the cap and the box gives
            // the logo's room up (the line then scrolls and fades as ever).
            let mut fit = LyricFit::default();
            fit.set_min(190.0);
            fit.line(Some(900.0), t0, robot, LYRIC_MAX_W);
            let cap = lyric_cap(robot, 190.0, LYRIC_MAX_W);
            assert_eq!(fit.width(), cap);
            let (plain_box, plain_span) = (fit.box_w(), fit.scroll_span());
            fit.set_slot(true);
            assert_eq!(fit.width(), cap, "never above the cap");
            assert_eq!(fit.box_w(), plain_box - TOAST_SLOT_EXTRA);
            // The scroll plan follows the smaller box: it runs longer.
            let span = fit.scroll_span().expect("a long line scrolls");
            assert!(span.1 > plain_span.expect("scrolls without the slot").1);
            fit.set_slot(false);
            assert_eq!(fit.scroll_span(), plain_span);
        }
    }

    #[test]
    fn lyric_fit_box_with_the_robot_is_todays_at_the_clamps() {
        let t0 = Instant::now();
        // Before `COMPACT_INSET` the robot's line box was the notch, clamped
        // to [floor, cap], less 40 + 20.
        for (min_w, max_w, text_w, today) in [
            (0.0, LYRIC_MAX_W, Some(900.0), LYRIC_MAX_W - 60.0),
            (0.0, LYRIC_MAX_W, Some(10.0), LYRIC_MIN_W - 60.0),
            (0.0, LYRIC_MAX_W, None, LYRIC_MIN_W - 60.0),
            (300.0, 250.0, Some(400.0), 300.0 - 60.0),
            (300.0, 250.0, Some(10.0), 300.0 - 60.0),
            (0.0, 450.0, Some(900.0), 450.0 - 60.0),
        ] {
            let mut fit = LyricFit::default();
            fit.set_min(min_w);
            fit.line(text_w, t0, true, max_w);
            assert_eq!(
                fit.box_w(),
                today,
                "line {text_w:?} min {min_w} max {max_w}"
            );
            // A refit of the same line (the robot off and on) lands on the
            // same width as the line itself.
            let width = fit.width();
            fit.refit(false, max_w);
            fit.refit(true, max_w);
            assert_eq!(
                fit.width(),
                width,
                "refit {text_w:?} min {min_w} max {max_w}"
            );
            fit.set_min(min_w + 1.0);
            fit.set_min(min_w);
            assert_eq!(
                fit.width(),
                width,
                "set_min {text_w:?} min {min_w} max {max_w}"
            );
        }
    }

    #[test]
    fn only_a_long_lyric_line_scrolls_after_its_hold() {
        let t0 = Instant::now();
        let mut fit = LyricFit::default();
        fit.line(Some(300.0), t0, true, LYRIC_MAX_W);
        assert_eq!(fit.scroll_span(), None, "fits: no timer, no frames");
        assert_eq!(fit.scroll(t0 + Duration::from_secs(5)), 0.0);
        fit.line(Some(800.0), t0, true, LYRIC_MAX_W);
        let (start, end) = fit.scroll_span().expect("too long for 560");
        assert_eq!(start, t0 + Duration::from_millis(LYRIC_HOLD_MS));
        assert!(end > start);
        assert_eq!(fit.scroll(start), 0.0);
        let moved = fit.scroll(start + Duration::from_millis(500));
        assert!(moved > 0.0);
        let rest = fit.scroll(end);
        assert_eq!(fit.scroll(end + Duration::from_secs(10)), rest);
        // The next line freezes this one's scroll for its fade-out.
        let later = end + Duration::from_secs(1);
        fit.line(Some(200.0), later, true, LYRIC_MAX_W);
        assert_eq!(fit.prev_scroll, rest);
        assert_eq!(fit.scroll_span(), None);
    }

    fn date(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    #[test]
    fn calendar_asks_on_start_navigation_and_when_stale() {
        let t0 = Instant::now();
        let month = (date(1), date(31));
        let next = (date(5), date(12));
        let secs = Duration::from_secs;
        // Nothing asked yet (start).
        assert!(needs_fetch(None, None, month, t0));
        // Reopening the panel on the range just asked: wait.
        assert!(!needs_fetch(Some((month, t0)), None, month, t0 + secs(59)));
        // Navigation lands on another range: always ask, however recent.
        assert!(needs_fetch(Some((month, t0)), None, next, t0));
        // ...and back again, since the sync thread now holds the other range.
        assert!(needs_fetch(Some((next, t0)), Some(secs(1)), month, t0));
        // Same range after 60 s: ask, unless the data shown is fresher
        // (the sync thread's own 10-minute refresh).
        assert!(needs_fetch(Some((month, t0)), None, month, t0 + secs(60)));
        assert!(!needs_fetch(
            Some((month, t0)),
            Some(secs(10)),
            month,
            t0 + secs(61)
        ));
        assert!(needs_fetch(
            Some((month, t0)),
            Some(secs(70)),
            month,
            t0 + secs(61)
        ));
    }

    #[test]
    fn countdown_redraws_when_the_shown_second_changes() {
        let ms = Duration::from_millis;
        // 24:59.5 left shows 25:00 until 24:59.0, half a second away.
        assert_eq!(ms_to_next_second(ms(1_499_500)), 501);
        assert_eq!(ms_to_next_second(ms(1_500_000)), 1001);
        assert_eq!(ms_to_next_second(ms(1)), 2);
        assert_eq!(ms_to_next_second(ms(999)), 1000);
    }

    #[test]
    fn clock_redraws_just_after_the_minute_changes() {
        let at = |h, m, s, ms| NaiveTime::from_hms_milli_opt(h, m, s, ms).unwrap();
        assert_eq!(ms_to_next_minute(at(9, 41, 0, 0)), 60_001);
        assert_eq!(ms_to_next_minute(at(9, 41, 30, 250)), 29_751);
        assert_eq!(ms_to_next_minute(at(23, 59, 59, 999)), 2);
        // A leap second (1_000+ ms) waits like the last millisecond, never 0.
        assert_eq!(ms_to_next_minute(at(23, 59, 59, 1_500)), 2);
    }

    #[test]
    fn time_changes_and_wake_ups_are_clock_jumps() {
        assert!(is_clock_jump(WM_TIMECHANGE, WPARAM(0)));
        assert!(is_clock_jump(
            WM_POWERBROADCAST,
            WPARAM(PBT_APMRESUMEAUTOMATIC)
        ));
        assert!(is_clock_jump(
            WM_POWERBROADCAST,
            WPARAM(PBT_APMRESUMESUSPEND)
        ));
        // Going to sleep (PBT_APMSUSPEND) or a power-source change is not.
        assert!(!is_clock_jump(WM_POWERBROADCAST, WPARAM(0x0004)));
        assert!(!is_clock_jump(WM_POWERBROADCAST, WPARAM(0x000A)));
        assert!(!is_clock_jump(WM_TIMER, WPARAM(0)));
    }

    #[test]
    fn progress_timer_runs_only_on_the_music_page_while_playing() {
        let music = View::Panel(Page::Media);
        let at = |position_ms| {
            Some(Progress {
                position_ms,
                end_ms: 238_000,
            })
        };
        // Just past the position's next whole second.
        assert_eq!(progress_wait(&music, at(102_000), true), Some(1001));
        assert_eq!(progress_wait(&music, at(102_250), true), Some(751));
        assert_eq!(progress_wait(&music, at(102_999), true), Some(2));
        // Paused, at the end, or without a known length: no timer.
        assert_eq!(progress_wait(&music, at(102_000), false), None);
        assert_eq!(progress_wait(&music, at(238_000), true), None);
        assert_eq!(progress_wait(&music, None, true), None);
        // Other pages and views: the equalizer timer (compact) or nothing.
        for view in [
            View::Panel(Page::Calendar),
            View::Panel(Page::Pomodoro),
            View::MediaCompact,
            View::MediaLyric,
            View::Idle,
            View::Hidden,
        ] {
            assert_eq!(progress_wait(&view, at(102_000), true), None, "{view:?}");
        }
    }

    #[test]
    fn a_stale_system_reading_is_forgotten_and_the_next_one_snaps() {
        let t0 = Instant::now();
        let at = |s| t0 + Duration::from_secs(s);
        let reading = |cpu| SystemSample {
            kind: SampleKind::Page,
            cpu,
            cores: 8,
            ram_used: 50,
            ram_total: 100,
            drives: Vec::new(),
            procs: Vec::new(),
            uptime: Duration::ZERO,
        };
        let mut sys = SystemState::new(WarnMonitor::new(10, 90));
        // Nothing read yet: nothing to forget.
        assert!(!sys.forget_stale(t0));
        let take = |sys: &mut SystemState, cpu, now| {
            let s = reading(cpu);
            sys.history.push(s.cpu, now);
            sys.ease.update(&s, now);
            sys.sample = Some(s);
        };
        take(&mut sys, 20.0, t0);
        // Opened again within 10 s: the figures stay and glide on.
        assert!(!sys.forget_stale(at(10)));
        assert!(sys.sample.is_some() && sys.ease.figures(at(10)).is_some());
        // Later: dashes until a fresh reading, which shows without a glide.
        assert!(sys.forget_stale(at(11)));
        assert!(sys.sample.is_none() && sys.history.points().is_empty());
        assert_eq!(sys.ease.figures(at(11)), None);
        take(&mut sys, 80.0, at(12));
        assert!(!sys.ease.running(at(12)));
        assert_eq!(sys.ease.figures(at(12)).unwrap().cpu, 80.0);
        assert_eq!(sys.history.points(), [80.0]);
    }

    #[test]
    fn system_page_samples_only_while_it_shows() {
        // Only the System page of the open panel wants samples, and not
        // while the cleaner covers it.
        assert!(system_sample_wanted(&View::Panel(Page::System), false));
        assert!(!system_sample_wanted(&View::Panel(Page::System), true));
        for view in [
            View::Idle,
            View::Hidden,
            View::MediaCompact,
            View::PomodoroCompact,
            View::Panel(Page::Media),
            View::Panel(Page::Cli),
        ] {
            assert!(!system_sample_wanted(&view, false), "{view:?}");
        }
        use PagePlan::*;
        // Just shown: ask at once. Waiting on a sample or the 1 s: nothing.
        assert_eq!(system_page_plan(true, false, false), Request);
        assert_eq!(system_page_plan(true, false, true), Keep);
        assert_eq!(system_page_plan(true, true, false), Keep);
        // Gone: the one-shot dies; a sample still coming re-arms nothing.
        assert_eq!(system_page_plan(false, true, false), Stop);
        assert_eq!(system_page_plan(false, true, true), Stop);
        assert_eq!(system_page_plan(false, false, true), Keep);
        assert_eq!(system_page_plan(false, false, false), Keep);
        // Left before the first reply came: the next showing still starts
        // with a full reading (the counter restarts, `Keep` or not).
        assert_eq!(requests_while(true, 3), 3);
        assert_eq!(requests_while(false, 3), 0);
        assert_eq!(requests_while(false, 0), 0);
        // The warning check: every 5 min, sooner to confirm RAM's minute.
        assert_eq!(warn_wait_ms(None), 300_000);
        assert_eq!(warn_wait_ms(Some(Duration::from_secs(60))), 60_250);
        assert_eq!(warn_wait_ms(Some(Duration::ZERO)), 1_000);
        assert_eq!(warn_wait_ms(Some(Duration::from_secs(900))), 300_000);
        assert!(SYSTEM_PAGE_MS == 1_000 && TIMER_SYSTEM_PAGE != TIMER_SYSTEM_WARN);
    }

    #[test]
    fn a_press_is_the_up_to_down_edge_only() {
        let mut e = OutsidePress::default();
        assert!(!e.poll(false, false));
        assert!(e.poll(true, true), "the press");
        assert!(!e.poll(true, false), "held down: no second edge");
        assert!(!e.poll(false, false), "the release");
        assert!(e.poll(true, false), "the next press");
        // A button already down when the polling starts is not a click.
        e.reset(true);
        assert!(!e.poll(true, false));
        assert!(!e.poll(false, false));
        assert!(e.poll(true, false));
    }

    #[test]
    fn a_click_quicker_than_the_poll_still_counts() {
        let mut e = OutsidePress::default();
        // Down and up again between two polls: only the "pressed since" bit.
        assert!(e.poll(false, true));
        assert!(!e.poll(false, false));
    }

    #[test]
    fn a_press_the_island_took_is_not_outside() {
        let mut e = OutsidePress::default();
        e.press_inside();
        assert!(!e.poll(true, true), "the island's own press");
        assert!(!e.poll(false, false));
        // The next press elsewhere counts again.
        assert!(e.poll(true, true));
        assert!(!e.poll(false, false));
        // A quick click on the island, seen only by its bit.
        e.press_inside();
        assert!(!e.poll(false, true));
        // An island press the poll never saw does not excuse a later one.
        e.press_inside();
        assert!(!e.poll(false, false));
        assert!(e.poll(true, false));
        // Starting the poll forgets one too.
        e.press_inside();
        e.reset(false);
        assert!(e.poll(true, false));
    }

    #[test]
    fn swapped_buttons_read_the_physical_right_one() {
        assert_eq!(primary_button_vk(false), VK_LBUTTON);
        assert_eq!(primary_button_vk(true), VK_RBUTTON);
    }

    #[test]
    fn a_middle_click_needs_its_press_and_release_on_the_island() {
        assert!(middle_click(true, true, true));
        assert!(!middle_click(false, true, true), "pressed elsewhere");
        assert!(!middle_click(true, true, false), "let go elsewhere");
        assert!(!middle_click(true, false, true), "gestures off by now");
    }

    #[test]
    fn the_pointer_polls_slow_down_when_nobody_is_there() {
        assert_eq!(outside_poll_ms(0), OUTSIDE_MS);
        assert_eq!(outside_poll_ms(OUTSIDE_IDLE_MS), OUTSIDE_MS);
        assert_eq!(outside_poll_ms(OUTSIDE_IDLE_MS + 1), OUTSIDE_SLOW_MS);
        // The slack never exceeds half the interval, nor 30 ms.
        assert_eq!(timer_slack(OUTSIDE_MS), 20);
        assert_eq!(timer_slack(halo::FAST_MS), 25);
        assert_eq!(timer_slack(OUTSIDE_SLOW_MS), 30);
        // The display state: off pauses, on and dimmed do not.
        assert!(display_state_off(0));
        assert!(!display_state_off(1));
        assert!(!display_state_off(2));
    }

    #[test]
    fn the_rec_ticks_land_just_after_the_second_changes() {
        let ms = Duration::from_millis;
        assert_eq!(ms_to_next_whole_second(ms(0)), 1001);
        assert_eq!(ms_to_next_whole_second(ms(250)), 751);
        assert_eq!(ms_to_next_whole_second(ms(999)), 2);
        assert_eq!(ms_to_next_whole_second(ms(61_400)), 601);
    }

    #[test]
    fn the_rec_timers_run_only_while_the_view_shows_and_moves() {
        // (shown, paused, motion_on, away) -> (clock, level)
        let case = |shown, paused, motion, away| {
            (
                rec_clock_timer_wanted(shown, paused, away),
                rec_level_timer_wanted(shown, paused, motion, away),
            )
        };
        assert_eq!(case(true, false, true, false), (true, true));
        // Motion Off keeps the clock, not the meters.
        assert_eq!(case(true, false, false, false), (true, false));
        // Paused: nothing moves.
        assert_eq!(case(true, true, true, false), (false, false));
        // Another view (or fullscreen's Hidden), locked, display off.
        assert_eq!(case(false, false, true, false), (false, false));
        assert_eq!(case(true, false, true, true), (false, false));
    }

    /// The body of `fn <name>` in this file, up to the next `fn` at the
    /// same depth.
    fn body_of(name: &str) -> &'static str {
        let src = include_str!("app.rs");
        let start = src.find(&format!("    fn {name}(")).expect(name);
        let rest = &src[start + 4..];
        let end = rest.find("\n    fn ").or_else(|| rest.find("\r\n    fn ")).unwrap_or(rest.len());
        &rest[..end]
    }

    /// Locking or the display going off re-plans the REC timers like the
    /// other polls (they then stop), and unlocking starts them again; the
    /// meters' timer is a coalescable one-shot re-armed after each redraw.
    #[test]
    fn the_rec_timers_follow_the_lock_and_the_meter_timer_is_a_one_shot() {
        assert!(body_of("sync_pointer_polls").contains("self.sync_rec_timers("));
        let arm = body_of("arm_rec_level");
        assert!(arm.contains("SetCoalescableTimer") && arm.contains("TIMER_REC_LEVEL"));
        let tick = body_of("on_timer");
        let at = tick.find("TIMER_REC_LEVEL =>").expect("the meter tick");
        let arm = &tick[at..at + tick[at..].find("TIMER_DAY").unwrap()];
        assert!(arm.contains("KillTimer") && arm.contains("self.arm_rec_level()"), "{arm}");
    }

    #[test]
    fn timer_ids_never_collide() {
        // The System page's and the clipboard's timers came in separate runs;
        // one id for two timers would kill or fire the other's.
        let ids = [
            TIMER_SYSTEM,
            TIMER_BATTERY,
            TIMER_DEADLINE,
            TIMER_EQ,
            TIMER_POMO,
            TIMER_DAY,
            TIMER_REC,
            TIMER_REC_LEVEL,
            TIMER_SCREEN_COUNTDOWN,
            TIMER_TRANSCRIBE,
            TIMER_CAPTION_LINE,
            TIMER_CLOCK,
            TIMER_REMINDER,
            TIMER_LYRIC,
            TIMER_SHORTCUT,
            TIMER_PROGRESS,
            TIMER_CLI,
            TIMER_BLINK,
            TIMER_ROBOT,
            TIMER_LYRIC_SCROLL,
            TIMER_SYSTEM_PAGE,
            TIMER_SYSTEM_WARN,
            clip::TIMER_CLIP_RETRY,
            clip::TIMER_CLIP_SAVE,
            clip::TIMER_CLIP_HOLD,
            TIMER_NOTIF_SAVE,
            shot::TIMER_SHOT_RETRY,
            shot::TIMER_SHOT_SHOW,
            TIMER_OUTSIDE,
            TIMER_MIXER,
            TIMER_VOLUME,
            TIMER_SHELF_HOLD,
            TIMER_BEHIND,
            TIMER_HALO,
            TIMER_LYRIC_RETRY,
            TIMER_DEVICES,
            TIMER_POINTER,
            TIMER_TYPING,
            TIMER_DISPLAYS,
            TIMER_PHONE,
            TIMER_WIDGET_SAVE,
            crate::widgets::host::TIMER_WIDGET_CARET,
            TIMER_STICKY_REMIND,
            crate::widgets::list_win::TIMER_LIST_UNDO,
            clip::TIMER_CLIP_DRAG,
            TIMER_MASCOT_PLAY,
        ];
        assert_eq!(clip::TIMER_CLIP_DRAG, 55);
        assert_eq!(TIMER_PHONE, 48);
        assert_eq!(crate::widgets::list_win::TIMER_LIST_UNDO, 49);
        // 42-47 are taken by other runs (the lyrics card's and the widgets'
        // ids); the phone never reuses one of them.
        assert!(!(42..48).contains(&TIMER_PHONE));
        assert_eq!(TIMER_WIDGET_SAVE, 45);
        assert_eq!(crate::widgets::host::TIMER_WIDGET_CARET, 46);
        assert_eq!(TIMER_STICKY_REMIND, 47);
        assert_eq!(TIMER_POINTER, 33);
        assert_eq!(TIMER_NOTIF_SAVE, 34);
        assert_eq!(TIMER_TYPING, 40);
        assert_eq!(TIMER_DISPLAYS, 41);
        for (i, a) in ids.iter().enumerate() {
            assert!(!ids[i + 1..].contains(a), "timer id {a} used twice");
        }
    }

    #[test]
    fn the_mascot_play_timer_has_its_own_id() {
        assert_eq!(TIMER_MASCOT_PLAY, 19);
    }

    /// An `Env` from 16 bits, one per field, in declaration order.
    fn env_from_bits(bits: u32) -> crate::ui::play::gate::Env {
        let b = |i: u32| bits & (1 << i) != 0;
        crate::ui::play::gate::Env {
            mascot_on: b(0),
            motion_full: b(1),
            energy_saver: b(2),
            win_anim: b(3),
            idle_view: b(4),
            panel_open: b(5),
            menu_open: b(6),
            fullscreen: b(7),
            screen_away: b(8),
            shrunk: b(9),
            pointer_near: b(10),
            asleep: b(11),
            recording: b(12),
            clock_on: b(13),
            has_story: b(14),
            working: b(15),
        }
    }

    #[test]
    fn the_play_timer_runs_only_when_a_play_could_start() {
        use crate::theme::MascotPlay::{Jarang, Mati, Sapaan, Sering, Sesekali};
        use crate::ui::play::gate::may_start;
        use crate::ui::play::trigger::Moment;
        let (since_input, since_last) = (Duration::from_secs(60), Duration::from_secs(3600));
        let mut armed = 0u32;
        for preset in [Sesekali, Sering, Jarang, Sapaan, Mati] {
            for bits in 0..(1u32 << 16) {
                let env = env_from_bits(bits);
                for playing in [false, true] {
                    // Written out, not read from the gates: the timer's own rule.
                    let timed = matches!(preset, Sesekali | Sering | Jarang);
                    let expected = timed
                        && !playing
                        && env.mascot_on
                        && env.motion_full
                        && !env.energy_saver
                        && env.win_anim
                        && env.idle_view
                        && !env.panel_open
                        && !env.menu_open
                        && !env.fullscreen
                        && !env.screen_away
                        && !env.shrunk
                        && !env.asleep
                        && !env.recording
                        && env.has_story
                        && !env.working;
                    let got = mascot_timer_wanted(&env, preset, playing);
                    assert_eq!(got, expected, "{preset:?} playing={playing} env={env:?}");
                    // It also agrees with the gate the fire re-checks, with the
                    // pointer's stale reading and the presence and cooldown (both
                    // decided when it fires) taken out.
                    let calm = crate::ui::play::gate::Env { pointer_near: false, ..env };
                    assert_eq!(
                        got,
                        !playing && may_start(&calm, Moment::Idle, preset, since_input, since_last),
                        "{preset:?} playing={playing} env={env:?}"
                    );
                    armed += u32::from(got);
                }
            }
        }
        assert!(armed > 0, "no combination arms it: the test checks nothing");
    }

    #[test]
    fn the_play_timer_is_armed_once_and_killed_when_a_gate_flips() {
        use crate::theme::MascotPlay::Sesekali;
        use crate::ui::play::testing::ok_env;
        let mut armed = false;
        let mut ops = Vec::new();
        let sync = |env: &crate::ui::play::gate::Env,
                    playing: bool,
                    armed: &mut bool,
                    ops: &mut Vec<bool>| {
            if let Some(op) = timer_change(*armed, mascot_timer_wanted(env, Sesekali, playing)) {
                *armed = op;
                ops.push(op);
            }
        };
        let ok = ok_env();
        for _ in 0..3 {
            sync(&ok, false, &mut armed, &mut ops);
        }
        assert_eq!(ops, [true], "armed once, never re-armed while it holds");
        let fullscreen = crate::ui::play::gate::Env { fullscreen: true, ..ok };
        for _ in 0..2 {
            sync(&fullscreen, false, &mut armed, &mut ops);
        }
        assert_eq!(ops, [true, false], "killed when a gate stops holding");
        sync(&ok, false, &mut armed, &mut ops);
        // A play starts: the timer that started it is gone and stays gone.
        for _ in 0..2 {
            sync(&ok, true, &mut armed, &mut ops);
        }
        assert_eq!(ops, [true, false, true, false]);
        assert!(!armed);
    }

    #[test]
    fn a_play_that_must_stop_instantly_lands_at_rest_in_one_step() {
        use crate::ui::play::state::Play;
        use crate::ui::play::testing::ok_env;
        use crate::ui::play::trigger::{Moment, Story};
        for (name, env) in [
            ("energy saver", crate::ui::play::gate::Env { energy_saver: true, ..ok_env() }),
            ("windows animations", crate::ui::play::gate::Env { win_anim: false, ..ok_env() }),
            ("motion", crate::ui::play::gate::Env { motion_full: false, ..ok_env() }),
        ] {
            let t0 = Instant::now();
            let mut play = Play::start(Story::Kelelawar, t0);
            let at = t0 + Duration::from_secs(1);
            assert!(step_play(&env, &mut play, Moment::Idle, false, false, at), "{name}");
            let b = play.beat(at);
            assert_eq!((b.spread, b.alpha), (0.0, 0.0), "{name}: not at rest");
        }
    }

    #[test]
    fn a_manual_play_is_not_killed_by_the_pointer_on_its_first_tick() {
        use crate::ui::play::state::Play;
        use crate::ui::play::testing::ok_env;
        use crate::ui::play::trigger::{Moment, Story};
        let near = crate::ui::play::gate::Env { pointer_near: true, menu_open: true, ..ok_env() };
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut manual = Play::start(Story::Kelelawar, t0);
        assert!(!step_play(&near, &mut manual, Moment::Manual, false, false, t0 + ms(100)));
        // Had it been ended at 100 ms it would be fully wound down by 300 ms.
        assert_eq!(manual.beat(t0 + ms(300)).alpha, 1.0, "the manual play was ended");
        assert!(manual.beat(t0 + ms(300)).spread > 0.9);
        // The same env ends an automatic play, and it eases home over 200 ms.
        let mut auto = Play::start(Story::Kelelawar, t0);
        assert!(!step_play(&near, &mut auto, Moment::Idle, false, false, t0 + ms(100)));
        assert!(auto.beat(t0 + ms(300)).alpha < 1.0, "the automatic play was not ended");
        assert!(step_play(&near, &mut auto, Moment::Idle, false, false, t0 + ms(300)));
    }

    #[test]
    fn a_changed_island_or_a_jumped_clock_ends_a_running_play() {
        use crate::ui::play::state::Play;
        use crate::ui::play::testing::ok_env;
        use crate::ui::play::trigger::{Moment, Story};
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        for (shape, jump) in [(true, false), (false, true)] {
            let mut p = Play::start(Story::Kelelawar, t0);
            assert!(!step_play(&ok_env(), &mut p, Moment::Idle, shape, jump, t0 + ms(100)));
            assert!(p.beat(t0 + ms(300)).alpha < 1.0);
        }
        let mut p = Play::start(Story::Kelelawar, t0);
        assert!(!step_play(&ok_env(), &mut p, Moment::Idle, false, false, t0 + ms(100)));
        assert_eq!(p.beat(t0 + ms(300)).alpha, 1.0, "nothing wrong: it plays on");
    }

    /// A story just started at `t0` for the Notch style.
    fn slot_at(t0: Instant) -> (Option<ActivePlay>, PlayShape) {
        use crate::theme::Mascot;
        use crate::ui::play::state::Play;
        use crate::ui::play::trigger::Story;
        let size = NotchSize::from_config(&Config::default());
        let shape = play_shape_of(Style::Notch, size, Mascot::Cat);
        let play = Play::start(Story::Kelelawar, t0);
        (Some(ActivePlay { play, moment: Moment::Idle, shape }), shape)
    }

    #[test]
    fn however_a_play_ends_the_window_is_not_click_through_on_that_frame() {
        use crate::theme::Mascot;
        use crate::ui::play::gate::Env;
        use crate::ui::play::testing::ok_env;
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let size = NotchSize::from_config(&Config::default());
        let other = play_shape_of(Style::Pill, size, Mascot::Cat);
        // (name, env, island changed, clock jumped, clicked)
        let paths = [
            ("done", ok_env(), false, false, false),
            ("finish_now", Env { energy_saver: true, ..ok_env() }, false, false, false),
            ("config change", ok_env(), true, false, false),
            ("clock jump", ok_env(), false, true, false),
            ("click", ok_env(), false, false, true),
        ];
        for (name, env, changed, jumped, clicked) in paths {
            let (mut slot, shape) = slot_at(t0);
            let frame_shape = if changed { other } else { shape };
            // With a cursor outside the island: the flag is up while it plays.
            let first = advance_slot(&mut slot, &env, frame_shape, jumped, clicked, false, t0 + ms(100));
            if slot.is_some() {
                assert!(first.click_through, "{name}: a running play, cursor outside, must pass clicks");
            }
            let mut dropped = slot.is_none();
            let mut t = 100;
            while !dropped {
                t += 100;
                assert!(t < 60_000, "{name}: the play never ended");
                let out = advance_slot(&mut slot, &env, frame_shape, false, false, false, t0 + ms(t));
                dropped = slot.is_none();
                assert_eq!(out.finished.is_some(), dropped, "{name}");
                assert_eq!(
                    out.click_through,
                    !dropped,
                    "{name}: the frame that drops the play must clear the flag, and only that one"
                );
            }
            // With no play left, nothing ever sets it again.
            let after = advance_slot(&mut slot, &ok_env(), frame_shape, false, false, false, t0 + ms(70_000));
            assert!(!after.click_through && after.finished.is_none(), "{name}");
        }
    }

    #[test]
    fn the_cursor_on_the_island_keeps_it_clickable_during_a_play() {
        use crate::ui::play::testing::ok_env;
        let t0 = Instant::now();
        let (mut slot, shape) = slot_at(t0);
        let out = advance_slot(&mut slot, &ok_env(), shape, false, false, true, t0 + Duration::from_millis(100));
        assert!(slot.is_some() && !out.click_through);
        let out = advance_slot(&mut slot, &ok_env(), shape, false, false, false, t0 + Duration::from_millis(200));
        assert!(out.click_through, "back outside it: click-through again");
    }

    #[test]
    fn a_button_down_ends_a_running_play_and_does_nothing_without_one() {
        use crate::ui::play::testing::ok_env;
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let (mut clicked, shape) = slot_at(t0);
        advance_slot(&mut clicked, &ok_env(), shape, false, false, false, t0 + ms(100));
        advance_slot(&mut clicked, &ok_env(), shape, false, true, false, t0 + ms(150));
        let beat = |slot: &Option<ActivePlay>, at| slot.as_ref().unwrap().play.beat(t0 + at);
        assert!(beat(&clicked, ms(300)).alpha < 1.0, "the click began the wind-down");
        let (mut calm, shape) = slot_at(t0);
        advance_slot(&mut calm, &ok_env(), shape, false, false, false, t0 + ms(150));
        assert_eq!(beat(&calm, ms(300)).alpha, 1.0, "no click: it plays on");
        let mut none = None;
        let out = advance_slot(&mut none, &ok_env(), shape, false, true, false, t0);
        assert!(none.is_none() && out.finished.is_none() && !out.click_through);
    }

    #[test]
    fn the_island_changing_its_look_is_a_changed_shape() {
        use crate::theme::Mascot;
        let size = NotchSize::from_config(&Config::default());
        let base = play_shape_of(Style::Notch, size, Mascot::Cat);
        assert_eq!(base, play_shape_of(Style::Notch, size, Mascot::Cat));
        let bigger = NotchSize { w: size.w + 10.0, ..size };
        let taller = NotchSize { h: size.h + 10.0, ..size };
        for (name, other) in [
            ("style", play_shape_of(Style::Pill, size, Mascot::Cat)),
            ("width", play_shape_of(Style::Notch, bigger, Mascot::Cat)),
            ("height", play_shape_of(Style::Notch, taller, Mascot::Cat)),
            ("mascot", play_shape_of(Style::Notch, size, Mascot::Falk)),
        ] {
            assert_ne!(base, other, "{name} did not count as a change");
        }
    }

    fn greeting_view() -> View {
        View::Notice(Notice {
            key: "bata-greeting".into(),
            source: "Vale".into(),
            title: String::new(),
            body: String::new(),
            kind: NoticeKind::Greeting { period: DayPeriod::Morning },
        })
    }

    #[test]
    fn the_greeting_moment_fires_once_when_the_greeting_notice_ends() {
        let mut up = false;
        let mut fired = Vec::new();
        for (i, view) in [
            View::Idle,
            greeting_view(),
            greeting_view(),
            View::Idle,
            View::Idle,
            greeting_view(),
            View::Notice(island_notice("t", "title", "body")),
        ]
        .iter()
        .enumerate()
        {
            if greeting_ended(&mut up, view) {
                fired.push(i);
            }
        }
        // Ends at steps 3 (back to idle) and 6 (another notice took over).
        assert_eq!(fired, [3, 6]);
        let mut up = false;
        assert!(!greeting_ended(&mut up, &View::Notice(island_notice("t", "a", "b"))));
        assert!(!greeting_ended(&mut up, &View::Idle));
    }

    #[test]
    fn a_greeting_play_waits_for_the_morph_and_starts_once_the_island_is_idle() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        // The notice just ended: a 400 ms morph back to the idle shape starts.
        assert_eq!(morph_left(true, t0, 400, t0), Some(ms(400)));
        assert_eq!(morph_left(true, t0, 400, t0 + ms(150)), Some(ms(250)));
        assert_eq!(morph_left(true, t0, 400, t0 + ms(399)), Some(ms(1)));
        // Reached its shape: at the end of the morph, or with none running.
        assert_eq!(morph_left(true, t0, 400, t0 + ms(400)), None);
        assert_eq!(morph_left(false, t0, 400, t0), None);
        assert_eq!(morph_left(false, t0, 400, t0 + ms(100)), None);
    }

    #[test]
    fn a_deferred_play_of_either_moment_waits_out_the_morph_and_never_starts_inside_it() {
        let ms = Duration::from_millis;
        // The panel just shut: its morph has 250 ms left.
        assert_eq!(settle_plan(Some(ms(250)), true), Settle::Wait(ms(251)));
        assert_ne!(settle_plan(Some(ms(250)), true), Settle::Now);
        assert_eq!(settle_plan(Some(ms(1)), true), Settle::Wait(ms(2)));
        // Settled: it starts at once (`try_play` still checks every gate).
        assert_eq!(settle_plan(None, true), Settle::Now);
        // Nothing may start, so nothing waits for a fire.
        assert_eq!(settle_plan(Some(ms(250)), false), Settle::Never);
    }

    #[test]
    fn the_hour_moment_fires_when_the_hour_rolls_over_and_not_within_it() {
        assert!(hour_rolled(9, 10));
        assert!(hour_rolled(23, 0));
        assert!(!hour_rolled(10, 10));
        assert!(!hour_rolled(0, 0));
    }

    #[test]
    fn the_cooldown_counts_from_the_last_play_and_a_first_play_has_none() {
        use crate::theme::MascotPlay::Sesekali;
        use crate::ui::play::gate::may_start;
        use crate::ui::play::testing::ok_env;
        use crate::ui::play::trigger::Moment;
        let now = Instant::now();
        let ok = Duration::from_secs(60);
        let try_at = |last: Option<Instant>| {
            may_start(&ok_env(), Moment::Idle, Sesekali, ok, since_last_play(last, now))
        };
        assert!(try_at(None), "no play yet: no cooldown");
        assert!(!try_at(Some(now - Duration::from_secs(60))));
        assert!(try_at(Some(now - Duration::from_secs(13 * 60))));
    }

    #[test]
    fn a_play_frame_is_never_skipped_as_unchanged_on_an_equalizer_tick() {
        // Compact media playing with live bars: nothing time-based, so a tick
        // with the same key is skipped...
        assert!(!eq_time_based(true, true, true, false));
        // ...unless a play is on that frame.
        assert!(eq_time_based(true, true, true, true));
        assert!(eq_time_based(false, false, false, true));
        // The sine (no live bars) stays time-based as before, and idle stays not.
        assert!(eq_time_based(true, true, false, false));
        assert!(!eq_time_based(true, false, false, false));
        assert!(!eq_time_based(false, true, false, false));
    }

    #[test]
    fn the_wheel_turns_the_volume_only_over_the_music() {
        let on = |view: &View, art| volume_wheel_applies(true, true, view, art, false);
        assert!(on(&View::MediaCompact, false));
        assert!(on(&View::MediaLyric, false));
        // The Music page: the cover turns the volume, the rest the page.
        assert!(on(&View::Panel(Page::Media), true));
        assert!(!on(&View::Panel(Page::Media), false));
        // Other pages and views keep the wheel for the pages.
        assert!(!on(&View::Panel(Page::Calendar), true));
        assert!(!on(&View::Idle, false));
        // A paused song (the caller passes `m.playing`, not that a song is
        // there) or nothing at all: the wheel turns pages. So does the
        // setting off.
        assert!(!volume_wheel_applies(
            true,
            false,
            &View::MediaCompact,
            false,
            false
        ));
        assert!(!volume_wheel_applies(
            false,
            true,
            &View::MediaCompact,
            false,
            false
        ));
    }

    #[test]
    fn a_paging_scroll_passing_over_the_cover_keeps_paging() {
        let t0 = Instant::now();
        let paging = |at| paging_recent(Some(t0), at);
        assert!(paging(t0 + Duration::from_millis(100)));
        assert!(!paging(t0 + PAGING_HOLD));
        assert!(!paging_recent(None, t0));
        let media = View::Panel(Page::Media);
        // Over the cover right after a page turned: the wheel stays a page.
        assert!(!volume_wheel_applies(true, true, &media, true, true));
        // A moment later the cover turns the volume again.
        assert!(volume_wheel_applies(true, true, &media, true, false));
        // The compact notch is the volume's whatever the pages did.
        assert!(volume_wheel_applies(
            true,
            true,
            &View::MediaCompact,
            false,
            true
        ));
    }

    #[test]
    fn background_readings_do_not_redraw_a_closed_island() {
        // A drain of only system readings draws nothing more.
        assert!(!drain_refreshes(1, 1, false));
        assert!(!drain_refreshes(2, 2, false));
        // Unless one raised a notice, or other events came too.
        assert!(drain_refreshes(1, 1, true));
        assert!(drain_refreshes(3, 1, false));
        assert!(drain_refreshes(0, 0, false), "a plain wake still refreshes");
    }

    #[test]
    fn eq_timer_runs_only_for_playing_compact_media_on_a_visible_screen() {
        // (compact, lyric, dance, playing, eyes, fullscreen, away)
        for bits in 0..128u32 {
            let b = |n: u32| bits & (1 << n) != 0;
            let (compact, lyric, dance) = (b(0), b(1), b(2));
            let (playing, eyes, fullscreen, away) = (b(3), b(4), b(5), b(6));
            let expect = (compact || (lyric && dance)) && playing && !eyes && !fullscreen && !away;
            assert_eq!(
                eq_timer_wanted(compact, lyric, dance, playing, eyes, fullscreen, away),
                expect,
                "compact {compact} lyric {lyric} dance {dance} playing {playing} eyes {eyes} fullscreen {fullscreen} away {away}"
            );
        }
        assert!(eq_timer_wanted(true, false, false, true, false, false, false));
        // Each condition alone stops it.
        assert!(
            !eq_timer_wanted(false, false, true, true, false, false, false),
            "another view"
        );
        assert!(
            !eq_timer_wanted(true, false, false, false, false, false, false),
            "paused"
        );
        assert!(
            !eq_timer_wanted(true, false, false, true, true, false, false),
            "eyes"
        );
        assert!(
            !eq_timer_wanted(true, false, false, true, false, true, false),
            "fullscreen"
        );
        assert!(
            !eq_timer_wanted(true, false, false, true, false, false, true),
            "locked or display off"
        );
        // The lyric view only while the cat dances there.
        assert!(eq_timer_wanted(false, true, true, true, false, false, false));
        assert!(
            !eq_timer_wanted(false, true, false, true, false, false, false),
            "lyric view without the dance"
        );
        assert!(
            !eq_timer_wanted(false, true, true, false, false, false, false),
            "lyric view, paused"
        );
        assert!(
            !eq_timer_wanted(false, true, true, true, true, false, false),
            "lyric view, shrunk to the eyes"
        );
    }

    #[test]
    fn eq_capture_needs_the_timer_and_the_setting() {
        assert!(eq_capture_wanted(true, true));
        assert!(!eq_capture_wanted(true, false), "setting off: never");
        assert!(!eq_capture_wanted(false, true), "no timer, no capture");
        assert!(!eq_capture_wanted(false, false));
        // End to end over every state: capture only with a running timer.
        for bits in 0..256u32 {
            let b = |n: u32| bits & (1 << n) != 0;
            let timer = eq_timer_wanted(b(0), b(1), b(2), b(3), b(4), b(5), b(6));
            let capture = eq_capture_wanted(timer, b(7));
            assert_eq!(capture, timer && b(7), "{bits:08b}");
            if !b(7) {
                assert!(!capture, "setting off never captures");
            }
        }
    }

    #[test]
    fn clock_timer_runs_only_while_the_idle_clock_shows() {
        assert!(clock_timer_wanted(&View::Idle, true));
        assert!(!clock_timer_wanted(&View::Idle, false), "setting off");
        let others = [
            View::Hidden,
            View::MediaCompact,
            View::PomodoroCompact,
            View::Panel(Page::Calendar),
            View::Notice(island_notice("t", "title", "body")),
        ];
        for view in others {
            assert!(!clock_timer_wanted(&view, true), "{view:?}");
        }
    }

    #[test]
    fn robot_timers_run_only_while_a_robot_shows() {
        let plan = |view: &View, on, motion, animating| {
            plan_timers(quiet_face(view, on), motion, animating)
        };
        let both = robot::TimerPlan {
            blink: true,
            change: true,
        };
        let none = robot::TimerPlan {
            blink: false,
            change: false,
        };
        let reminder = Notice {
            kind: NoticeKind::Reminder { id: "r".into() },
            ..island_notice("r", "t", "b")
        };
        // Compact views and the island's own notices: the one-shot, and a
        // blink where the face has open eyes (not the happy music faces).
        for (view, blinks) in [
            (View::Idle, true),
            (View::PomodoroCompact, true),
            (View::Notice(reminder), true),
            (View::MediaCompact, false),
            (View::MediaLyric, false),
        ] {
            let full = robot::TimerPlan {
                blink: blinks,
                ..both
            };
            assert_eq!(plan(&view, true, Motion::Full, false), full, "{view:?}");
            // Saver: no blink, sleep still counts. Off: nothing at all.
            let saver = plan(&view, true, Motion::Saver, false);
            assert_eq!((saver.blink, saver.change), (false, true));
            assert_eq!(plan(&view, true, Motion::Off, false), none);
            // Robot off: as before, no robot timers.
            assert_eq!(plan(&view, false, Motion::Full, false), none);
            // The frame clock draws anyway: no blink meanwhile.
            assert!(!plan(&view, true, Motion::Full, true).blink);
        }
        // Asleep (or happy): one-shot only, no blink; an idle island then
        // runs no timer at all until something happens.
        let asleep = plan_timers(Some(robot::Expr::Sleep), Motion::Full, false);
        assert_eq!((asleep.blink, asleep.change), (false, true));
        // No robot: the panel, fullscreen, other apps' notifications.
        for view in [
            View::Hidden,
            View::Panel(Page::Calendar),
            View::Notice(island_notice("t", "title", "body")),
        ] {
            assert_eq!(plan(&view, true, Motion::Full, false), none, "{view:?}");
        }
    }

    #[test]
    fn activity_is_what_changes_something() {
        for event in [
            Event::Media(None),
            Event::Notice(island_notice("t", "title", "body")),
            Event::ClaudeActivity {
                session_id: "s".into(),
            },
            Event::Click(Hit::Body),
            Event::Wheel(-120),
            Event::Hover(true),
        ] {
            assert!(is_activity(&event), "{event:?}");
        }
        for event in [
            Event::Tick,
            Event::Hover(false),
            Event::Fullscreen(true),
            Event::MediaTimeline(None),
            Event::LyricsReady(true),
            Event::Today(date(27)),
        ] {
            assert!(!is_activity(&event), "{event:?}");
        }
    }

    #[test]
    fn greeting_counts_follow_the_period() {
        use crate::calendar::EventTime;
        use chrono::TimeZone;
        let all_day = |id: &str, d: u32| CalEvent {
            id: id.into(),
            calendar_id: "c".into(),
            title: id.into(),
            start: EventTime::AllDay(date(d)),
            end: EventTime::AllDay(date(d + 1)),
            description: String::new(),
            color: None,
            editable: true,
        };
        let stamp = |d: u32, h: u32, m: u32| {
            EventTime::At(
                Local
                    .from_local_datetime(&date(d).and_hms_opt(h, m, 0).unwrap())
                    .earliest()
                    .unwrap(),
            )
        };
        let timed = |id: &str, d: u32, from: (u32, u32), to: (u32, u32)| CalEvent {
            start: stamp(d, from.0, from.1),
            end: stamp(d, to.0, to.1),
            ..all_day(id, d)
        };
        let events = [
            all_day("a", 27),
            all_day("b", 27),
            all_day("c", 28),
            timed("over", 27, (9, 0), (10, 0)),
            timed("running", 27, (11, 30), (12, 30)),
            timed("later", 27, (13, 0), (14, 0)),
            timed("tomorrow", 28, (9, 0), (10, 0)),
        ];
        let at = |d: u32, h: u32| local_unix(date(d), h, 0);
        let reminder = |id: &str, due: i64, done: bool| Reminder {
            id: id.into(),
            text: id.into(),
            due_unix: due,
            repeat: reminders::Repeat::Once,
            done,
            days: Vec::new(),
        };
        let list = [
            reminder("early", at(27, 7), false),
            reminder("later", at(27, 16), false),
            reminder("done", at(27, 9), true),
            reminder("tomorrow", at(28, 9), false),
        ];
        let now = |d: u32, h: u32, m: u32| date(d).and_hms_opt(h, m, 0).unwrap();
        let counts = |now, period| greeting_counts(&events, &list, now, period);
        let c = |events, reminders| GreetCounts { events, reminders };
        // Morning: all of today (5 events, the 2 undone reminders).
        assert_eq!(counts(now(27, 8, 0), DayPeriod::Morning), c(5, 2));
        // Midday and Afternoon: what is left. At 12:00 "over" is gone, the
        // 07:00 reminder too; all-day ones stay.
        assert_eq!(counts(now(27, 12, 0), DayPeriod::Midday), c(4, 1));
        assert_eq!(counts(now(27, 13, 30), DayPeriod::Midday), c(3, 1));
        assert_eq!(counts(now(27, 15, 0), DayPeriod::Afternoon), c(2, 1));
        assert_eq!(counts(now(27, 16, 0), DayPeriod::Afternoon), c(2, 1));
        assert_eq!(counts(now(27, 17, 0), DayPeriod::Afternoon), c(2, 0));
        // Evening: tomorrow's events; the all-day "c" and the timed one.
        assert_eq!(counts(now(27, 20, 0), DayPeriod::Evening), c(2, 0));
        // Night (after midnight): the current calendar date's events.
        assert_eq!(counts(now(28, 1, 0), DayPeriod::Night), c(2, 0));
        // Nothing at all.
        for period in [
            DayPeriod::Morning,
            DayPeriod::Midday,
            DayPeriod::Afternoon,
            DayPeriod::Evening,
            DayPeriod::Night,
        ] {
            assert_eq!(
                greeting_counts(&[], &[], now(27, 12, 0), period),
                GreetCounts::default(),
                "{period:?}"
            );
        }
    }

    #[test]
    fn a_page_later_in_the_owners_order_slides_in_from_the_right() {
        let mut pages = PanelPages::default();
        assert_eq!(slide_from(&pages, Page::Calendar, Page::Pomodoro), 1.0);
        assert_eq!(slide_from(&pages, Page::Pomodoro, Page::Calendar), -1.0);
        // Shortcuts moved to the front: going to it is going back.
        pages.move_to(Page::Shortcuts, 0);
        assert_eq!(slide_from(&pages, Page::Calendar, Page::Shortcuts), -1.0);
        assert_eq!(slide_from(&pages, Page::Shortcuts, Page::Calendar), 1.0);
    }

    #[test]
    fn page_slide_eases_out_to_rest() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let right = Slide {
            strip: SlideStrip::next(None, 0.0, Page::Calendar, 1.0),
            start: t0,
        };
        assert_eq!(right.offset(t0), 1.0);
        let mid = right.offset(ms(110));
        assert!((mid - 0.125).abs() < 1e-3, "half time, eased: {mid}");
        assert!(right.offset(ms(50)) > right.offset(ms(60)));
        assert_eq!(right.progress(ms(220)), 1.0);
        assert_eq!(right.offset(ms(220)), 0.0);
        assert_eq!(right.offset(ms(500)), 0.0);
        let left = Slide {
            strip: SlideStrip::next(None, 0.0, Page::Pomodoro, -1.0),
            start: t0,
        };
        assert_eq!(left.offset(t0), -1.0);
        assert!(left.offset(ms(110)) < 0.0);
    }

    #[test]
    fn the_last_page_fade_frame_registers_regions() {
        let t0 = Instant::now();
        let fade = PageFade {
            from_page: Page::Calendar,
            start: t0,
        };
        let at = |ms: u64| page_fade_frame(fade.progress(t0 + Duration::from_millis(ms)));
        assert!(at(PAGE_FADE_MS / 2).is_some());
        // The frame drawn when `animate` sees the fade done is a settled one:
        // the renderer gets no fade and registers the page's regions.
        assert_eq!(at(PAGE_FADE_MS), None);
        assert!(crate::ui::motion::panel_regions_wanted(
            true,
            false,
            0.0,
            at(PAGE_FADE_MS)
        ));
        assert!(!crate::ui::motion::panel_regions_wanted(
            true,
            false,
            0.0,
            at(PAGE_FADE_MS / 2)
        ));
    }

    #[test]
    fn a_slide_started_mid_slide_keeps_the_pages_where_they_are() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        // Calendar to Media; a fast spin turns again 20 ms in, while the
        // Calendar still covers most of the panel.
        let first = Slide {
            strip: SlideStrip::next(None, 0.0, Page::Calendar, 1.0),
            start: t0,
        };
        let offset = first.offset(ms(20));
        assert!(offset > 0.5 && offset < 1.0, "{offset}");
        // Where each page is drawn at `now`: the page coming in at the
        // offset, the pages going out from the strip.
        let drawn = |slide: &Slide, coming: Page, now: Instant| {
            let at = slide.offset(now);
            let mut pages: Vec<(Page, f32)> =
                slide.strip.visible(at).into_iter().flatten().collect();
            if at.abs() < 1.0 {
                pages.push((coming, at));
            }
            pages
        };
        let before = drawn(&first, Page::Media, ms(20));
        assert_eq!(before.len(), 2);
        for (dir, coming) in [(1.0, Page::Notes), (-1.0, Page::Calendar)] {
            let next = Slide {
                strip: SlideStrip::next(Some(&first.strip), offset, Page::Media, dir),
                start: ms(20),
            };
            // Every page on screen at the old slide's last frame is drawn at
            // the same place at the new slide's first.
            let after = drawn(&next, coming, ms(20));
            for (page, x) in &before {
                assert!(
                    after
                        .iter()
                        .any(|(p, at)| p == page && (at - x).abs() < 1e-5),
                    "dir {dir}: {page:?} at {x} is gone or moved: {after:?}"
                );
            }
            // And it ends at rest like any slide.
            assert_eq!(next.offset(ms(20 + SLIDE_MS as u64)), 0.0);
        }
    }

    #[test]
    fn a_morph_without_windows_animations_never_overshoots() {
        let t0 = Instant::now();
        let plain = Anim {
            curve: MorphCurve::Plain,
            ..morph(t0)
        };
        let target = plain.to;
        for ms in (0..=ANIM_MS).step_by(8) {
            let (shape, _) = plain.frame(t0 + Duration::from_millis(ms));
            assert!(
                shape.w <= target.w + 1e-3 && shape.h <= target.h + 1e-3,
                "{ms} ms"
            );
        }
        let overshoots = (0..=ANIM_MS).step_by(8).any(|ms| {
            let (shape, _) = morph(t0).frame(t0 + Duration::from_millis(ms));
            shape.h > target.h + 1e-3
        });
        assert!(
            overshoots,
            "with animations on, the spring still overshoots"
        );
        assert_eq!(plain.frame(t0 + Duration::from_millis(ANIM_MS)).0, target);
    }

    fn temp_notes(name: &str) -> (PathBuf, NotesStore) {
        let dir = std::env::temp_dir().join(format!("vale-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = NotesStore::open(dir.join("notes.json"));
        (dir, store)
    }

    #[test]
    fn only_changed_notes_are_saved() {
        let (dir, mut notes) = temp_notes("changed");
        let id = notes.upsert(None, "beli kopi", 1).unwrap();
        assert!(!note_changed(&notes, Some(&id), "beli kopi"), "untouched");
        assert!(note_changed(&notes, Some(&id), "beli teh"));
        assert!(note_changed(&notes, Some(&id), ""), "emptied: deletes");
        assert!(!note_changed(&notes, None, "  \n"), "empty new note");
        assert!(note_changed(&notes, None, "baru"));
        assert!(note_changed(&notes, Some("n0-gone"), "masih ada"));
        assert!(!dir.exists(), "nothing written");
    }

    #[test]
    fn unreadable_notes_file_is_announced() {
        let (dir, notes) = temp_notes("unreadable-none");
        assert_eq!(notes_unreadable_notice(&notes), None, "missing file");
        assert!(!dir.exists());

        let (dir, _) = temp_notes("unreadable");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.json"), "{ rusak").unwrap();
        let notes = NotesStore::open(dir.join("notes.json"));
        let notice = notes_unreadable_notice(&notes);
        let _ = std::fs::remove_dir_all(&dir);
        let notice = notice.expect("a notice");
        assert_eq!(notice.title, "notes.json tidak terbaca");
        assert_eq!(
            notice.body,
            "Disimpan sebagai notes.json.bak saat menyimpan berikutnya"
        );
    }

    #[test]
    fn note_editor_opens_the_note_or_a_new_one() {
        let (_dir, mut notes) = temp_notes("kind");
        let id = notes.upsert(None, "isi", 1).unwrap();
        assert_eq!(
            note_editor_kind(&notes, Some(&id)),
            EditorKind::Note {
                id: Some(id),
                text: "isi".into()
            }
        );
        let new = EditorKind::Note {
            id: None,
            text: String::new(),
        };
        assert_eq!(note_editor_kind(&notes, None), new);
        assert_eq!(note_editor_kind(&notes, Some("n0-gone")), new);
    }

    #[test]
    fn event_editor_is_read_only_unless_editable() {
        use crate::calendar::EventTime;
        let event = |id: &str, editable| CalEvent {
            id: id.into(),
            calendar_id: "team".into(),
            title: id.into(),
            start: EventTime::AllDay(date(25)),
            end: EventTime::AllDay(date(26)),
            description: String::new(),
            color: None,
            editable,
        };
        let events = [event("rw", true), event("ro", false)];
        let open = |id| match event_editor_kind(&events, id, date(27)) {
            Some(EditorKind::Event {
                event_id,
                calendar_id,
                draft,
                read_only,
            }) => (event_id, calendar_id, draft.date, read_only),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(
            open(Some("rw")),
            (Some("rw".into()), Some("team".into()), date(25), false)
        );
        assert_eq!(
            open(Some("ro")),
            (Some("ro".into()), Some("team".into()), date(25), true)
        );
        // New: on the selected date, writable, into the primary calendar.
        assert_eq!(open(None), (None, None, date(27), false));
        assert_eq!(event_editor_kind(&events, Some("gone"), date(27)), None);
    }

    fn local_unix(d: NaiveDate, h: u32, m: u32) -> i64 {
        reminders::local_unix(d, NaiveTime::from_hms_opt(h, m, 0).unwrap()).unwrap()
    }

    fn due_reminder(due: i64) -> Reminder {
        Reminder {
            id: "r1-0".into(),
            text: "Minum obat".into(),
            due_unix: due,
            repeat: reminders::Repeat::Daily,
            done: false,
            days: Vec::new(),
        }
    }

    #[test]
    fn reminder_notice_on_time_late_and_missed() {
        let due = local_unix(date(26), 8, 30);
        let on_time = reminder_notice(&due_reminder(due), due + 1, false);
        assert_eq!(on_time.title, "Minum obat");
        assert_eq!(on_time.body, "08.30");
        assert_eq!(on_time.source, "Pengingat");
        assert_eq!(on_time.key, "reminder-r1-0");
        assert_eq!(on_time.kind, NoticeKind::Reminder { id: "r1-0".into() });
        // Woken from sleep ten minutes late.
        let late = reminder_notice(&due_reminder(due), due + 600, false);
        assert_eq!(late.body, "Terlewat · 08.30");
        // At startup, even a just-passed one was missed while off.
        let missed = reminder_notice(&due_reminder(due), due + 5, true);
        assert_eq!(missed.body, "Terlewat · 08.30");
        // From another day: with its date.
        let next_day = local_unix(date(27), 9, 0);
        let old = reminder_notice(&due_reminder(due), next_day, true);
        assert_eq!(old.body, "Terlewat · 26 Okt 08.30");
    }

    #[test]
    fn reminder_notice_in_english() {
        i18n::set_lang(i18n::Lang::En);
        let due = local_unix(date(26), 8, 30);
        let missed = reminder_notice(&due_reminder(due), due + 5, true);
        assert_eq!(
            (missed.source.as_str(), missed.body.as_str()),
            ("Reminder", "Missed · 08:30")
        );
    }

    #[test]
    fn reminder_timer_waits_for_the_next_but_at_most_an_hour() {
        assert_eq!(reminder_wait_ms(None, 5_000), None);
        // Due in 90 s.
        assert_eq!(reminder_wait_ms(Some(100), 10_000), Some(90_001));
        // Already due (a moment ago): at once.
        assert_eq!(reminder_wait_ms(Some(100), 100_500), Some(1));
        // Tomorrow: an hour, then it looks again.
        assert_eq!(reminder_wait_ms(Some(86_400), 0), Some(3_600_000));
    }

    #[test]
    fn reminder_editor_opens_the_reminder_or_a_new_one_next_hour() {
        let dir =
            std::env::temp_dir().join(format!("vale-app-reminder-kind-{}", std::process::id()));
        let mut store = RemindersStore::open(dir.join("reminders.json"));
        let due = local_unix(date(27), 7, 15);
        let id = store
            .upsert(None, "Telepon ibu", due, reminders::Repeat::Weekly, &[], 1)
            .unwrap();
        let now = date(26).and_hms_opt(22, 40, 0).unwrap();
        // Saved without days: the weekday of its date (a Tuesday).
        assert_eq!(
            reminder_editor_kind(&store, Some(&id), now),
            EditorKind::Reminder {
                id: Some(id),
                text: "Telepon ibu".into(),
                date: date(27),
                time: NaiveTime::from_hms_opt(7, 15, 0).unwrap(),
                repeat: reminders::Repeat::Weekly,
                days: vec![chrono::Weekday::Tue],
            }
        );
        use chrono::Weekday::{Fri, Tue};
        let several = store
            .upsert(
                None,
                "Backup",
                due,
                reminders::Repeat::Weekly,
                &[Fri, Tue],
                1,
            )
            .unwrap();
        match reminder_editor_kind(&store, Some(&several), now) {
            EditorKind::Reminder { days, .. } => assert_eq!(days, [Tue, Fri]),
            other => panic!("{other:?}"),
        }
        let new = |now| match reminder_editor_kind(&store, Some("r0-gone"), now) {
            EditorKind::Reminder {
                id: None,
                text,
                date,
                time,
                repeat: reminders::Repeat::Once,
                days,
            } if text.is_empty() && days.is_empty() => (date, time),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            new(now),
            (date(26), NaiveTime::from_hms_opt(23, 0, 0).unwrap())
        );
        let late = date(26).and_hms_opt(23, 5, 0).unwrap();
        assert_eq!(new(late), (date(27), NaiveTime::MIN), "past midnight");
        assert!(!dir.exists(), "nothing written");
    }

    #[test]
    fn unreadable_reminders_file_is_announced() {
        let dir =
            std::env::temp_dir().join(format!("vale-app-reminders-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("reminders.json"), "[{").unwrap();
        let store = RemindersStore::open(dir.join("reminders.json"));
        let _ = std::fs::remove_dir_all(&dir);
        let notice = reminders_unreadable_notice(&store).expect("a notice");
        assert_eq!(notice.title, "reminders.json tidak terbaca");
        assert_eq!(
            reminders_unreadable_notice(&RemindersStore::open(dir.join("none.json"))),
            None
        );
    }

    #[test]
    fn a_hung_explorer_read_stops_blocking_the_next() {
        let t0 = Instant::now();
        let mut reads = ExplorerReads::default();
        assert!(!reads.busy(t0), "nothing running");
        reads.running = Some(1);
        reads.started = Some(t0);
        assert!(reads.busy(t0 + Duration::from_secs(1)));
        // A COM call that never comes back (a hung Explorer) is given up on;
        // a late answer is told apart by its request id.
        assert!(!reads.busy(t0 + EXPLORER_READ_TIMEOUT));
    }

    #[test]
    fn right_clicks_on_a_rec_row_open_its_menu() {
        let rec = View::Panel(Page::Record);
        assert_eq!(rec_row_menu_target(rec.clone(), Some(&Hit::RecOpen(2))), Some(2));
        assert_eq!(rec_row_menu_target(rec.clone(), Some(&Hit::RecTranscript(4))), Some(4));
        // Anything else on the page, or no hit: the tray menu, as before.
        assert_eq!(rec_row_menu_target(rec.clone(), Some(&Hit::RecFolder)), None);
        assert_eq!(rec_row_menu_target(rec.clone(), Some(&Hit::RecPickFile)), None);
        assert_eq!(rec_row_menu_target(rec, None), None);
        // Other pages never.
        let notes = View::Panel(Page::Notes);
        assert_eq!(rec_row_menu_target(notes, Some(&Hit::RecOpen(2))), None);
        assert_eq!(rec_row_menu_target(View::Panel(Page::Media), Some(&Hit::RecTranscript(0))), None);
    }

    #[test]
    fn right_clicks_on_a_video_row_open_its_menu() {
        use crate::model::{RecEntry, RowTranscript};
        let entry = |name: &str| RecEntry {
            path: PathBuf::from(name),
            title: name.into(),
            started_at: chrono::NaiveDateTime::default(),
            duration: Duration::from_secs(60),
            unfinished: false,
            silent: false,
            transcript: RowTranscript::None,
        };
        let t0 = Instant::now();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut s = IslandState::new(today, Default::default());
        s.apply(Event::RecRecent(vec![entry("a.m4a"), entry("b.m4a")], 2), t0);
        s.apply(Event::VideoRecent(vec![entry("a.mp4"), entry("b.mp4")], 2), t0);
        s.apply(Event::Hover(true), t0);
        let at = |p: Page| Page::ALL.iter().position(|q| *q == p).unwrap() as i32;
        s.apply(Event::Wheel(-120 * (at(Page::Record) - at(s.page()))), t0);
        assert_eq!(s.view(), View::Panel(Page::Record));
        // The menu's row is the row of the tab's list: an audio recording
        // on Audio, the video in the same place on Layar.
        let row = |s: &IslandState, hit: Hit| {
            rec_row_menu_target(s.view(), Some(&hit)).map(|i| s.rec_list()[i].path.clone())
        };
        assert_eq!(row(&s, Hit::RecOpen(1)), Some(PathBuf::from("b.m4a")));
        s.apply(Event::Click(Hit::RecTab(RecKind::Screen)), t0);
        assert_eq!(s.rec_tab(), RecKind::Screen);
        assert_eq!(row(&s, Hit::RecOpen(1)), Some(PathBuf::from("b.mp4")));
        assert_eq!(row(&s, Hit::RecTranscript(0)), Some(PathBuf::from("a.mp4")));
        // The switch and "Buang" have no row menu.
        assert_eq!(row(&s, Hit::RecTab(RecKind::Audio)), None);
        assert_eq!(row(&s, Hit::ScreenDiscard), None);
    }

    #[test]
    fn note_card_menu_target_only_on_a_notes_card() {
        let notes = View::Panel(Page::Notes);
        assert_eq!(
            note_card_menu_target(notes.clone(), Some(&Hit::NoteOpen("n1".into()))),
            Some("n1".into())
        );
        assert_eq!(
            note_card_menu_target(notes.clone(), Some(&Hit::NoteSticky("n1".into()))),
            Some("n1".into())
        );
        assert_eq!(note_card_menu_target(notes.clone(), Some(&Hit::NoteNew)), None);
        assert_eq!(note_card_menu_target(notes, None), None);
        assert_eq!(
            note_card_menu_target(
                View::Panel(Page::Clipboard),
                Some(&Hit::NoteOpen("n1".into()))
            ),
            None
        );
    }

    #[test]
    fn right_clicks_inside_the_folder_menu_never_open_the_tray_menu() {
        let path = PathBuf::from(r"D:\x");
        let pick = |favorite| Hit::CliFolderPick {
            path: path.clone(),
            favorite,
        };
        assert_eq!(
            menu_right_click(Some(pick(true))),
            MenuRightClick::Unstar(path.clone())
        );
        assert_eq!(menu_right_click(Some(pick(false))), MenuRightClick::Swallow);
        let star = Hit::CliFolderStar {
            path: path.clone(),
            on: true,
        };
        assert_eq!(menu_right_click(Some(star)), MenuRightClick::Swallow);
        // Outside the menu: the tray menu, as everywhere else.
        assert_eq!(
            menu_right_click(Some(Hit::CliMenuDismiss)),
            MenuRightClick::Tray
        );
        assert_eq!(menu_right_click(None), MenuRightClick::Tray);
    }

    #[test]
    fn connect_on_an_invalid_config_says_so() {
        let own = |id: &str| client::choose((id, "s"), ("", ""));
        let id = "id.apps.googleusercontent.com";
        // The ID may be in the broken file; "fill in the ID" would mislead.
        assert_eq!(connect_action(false, own("")), ConnectAction::InvalidConfig);
        assert_eq!(connect_action(false, own(id)), ConnectAction::InvalidConfig);
        assert_eq!(connect_action(true, own("  ")), ConnectAction::NoClientId);
        assert_eq!(connect_action(true, own(id)), ConnectAction::Login);
        // No own ID, but Vale's built-in client: log in with it.
        let built_in = client::choose(("", ""), ("vale.apps.googleusercontent.com", "v"));
        assert_eq!(connect_action(true, built_in), ConnectAction::Login);
    }

    #[test]
    fn invalid_config_notice_says_where_the_file_went() {
        let bak = PathBuf::from(r"C:\x\Vale\config.json.bak");
        let saved = invalid_config_notice(Some(&Repair::BackedUp(bak)));
        assert_eq!(saved.title, "config.json tidak valid");
        assert_eq!(saved.body, "Disalin ke config.json.bak — cek lagi isinya");
        let blocked = invalid_config_notice(Some(&Repair::Blocked));
        assert_eq!(blocked.title, "config.json tidak valid");
        assert_eq!(blocked.body, "Tidak bisa disalin — cek lagi isinya");
        let connect = invalid_config_notice(None);
        assert_eq!(connect.title, "config.json tidak valid");
        assert_eq!(
            connect.kind,
            NoticeKind::App {
                app_id: None,
                toast: None
            }
        );
    }

    #[test]
    fn island_notices_in_english() {
        i18n::set_lang(i18n::Lang::En);
        let bak = PathBuf::from(r"C:\x\Vale\config.json.bak");
        let saved = invalid_config_notice(Some(&Repair::BackedUp(bak)));
        assert_eq!(saved.title, "config.json is invalid");
        assert_eq!(saved.body, "Copied to config.json.bak — check its contents");
        let n = client_id_notice();
        assert_eq!(
            (n.source.as_str(), n.title.as_str()),
            ("Calendar", "Add a Google client ID first")
        );
        assert_eq!(dnd_notice().title, "Turn off Windows pop-ups");
    }

    #[test]
    fn client_id_notice_points_to_the_readme() {
        let n = client_id_notice();
        assert_eq!(n.source, "Kalender");
        assert_eq!(n.title, "Isi Google client ID dulu");
        assert_eq!(n.body, "Pengaturan → Integrasi, panduan ada di README");
        assert_eq!(
            n.kind,
            NoticeKind::App {
                app_id: None,
                toast: None
            }
        );
    }

    #[test]
    fn anim_midway_fades_linearly() {
        let t0 = Instant::now();
        let anim = morph(t0);
        let (_, opacity) = anim.frame(t0 + Duration::from_millis(ANIM_MS / 2));
        assert!((opacity - 0.5).abs() < 0.01, "opacity {opacity}");
    }
}
