//! `--demo` (debug builds only): cycles through every view, 2 s each, Pill
//! then Notch, with fake calendar, notes, reminders and pomodoro data (some
//! of it long, to check trimming), so the window and renderer can be checked
//! by eye and by screen capture against `docs/design/studio-ui.pdf`. Runs
//! until the process is killed.
//!
//! Environment knobs for screenshots:
//! `--demo-cli` runs only the CLI page's scenes, for comparing with
//! `docs/design/pengingat-cli.html` (p3, p4, p5) and the folder ones with
//! `docs/design/keputusan-v3.1.html` (p2). Their folder menu times count
//! from a fixed "now", so they never depend on the clock.
//!
//! - `DI_DEMO_STEP=n` holds step `n` (see `SCENES`; Notch adds `SCENES.len()`).
//!   A scene's name works too: `DI_DEMO_STEP=cal-day`, or `notch:cal-day`.
//! - `DI_DEMO_T=0.5` freezes the morph into that step at this progress.
//! - `DI_DEMO_SLIDE=0.4` draws the panel mid page-slide.
//! - `DI_DEMO_REVEAL=0.5` freezes a sub-view `Reveal` (the new content
//!   fading in over 160 ms, 8 px from where it rests) at that progress, in
//!   the scenes `cal-week-to-day` (from below) and `cleaner-open` (from
//!   below), and the page cross-fade (`PageFade`, 160 ms) in `search-open`
//!   (the search over the Music page, as the island opens it). Without it
//!   they play once from the step's start. While a scene morphs in from a panel, a compact view or
//!   notice fades in with the morph (`DI_DEMO_T=0.5` shows it halfway), and
//!   the panel it replaces fades out with the shrinking shape.
//! - `DI_DEMO_SHELF=0.5` freezes the shelf's card motion at that progress
//!   (0 to 1 of the scene's span: 260 ms for `shelf-enter`, 220 ms for
//!   `shelf-exit` and `shelf-move`). Without it the scene plays once from
//!   the step's start. `shelf-enter` has three more files dropped on a
//!   shelf of three (they stagger in), `shelf-exit` takes the second of six
//!   off (it shrinks and fades where it was, the rest slide over) and
//!   `shelf-move` takes the third of nine off (three rows become two, so
//!   every card changes place). Made-up names only.
//! - The phone link over the shelf (made-up data, no network, no server):
//!   `phone-qr` (the QR code of an invented link, with the cat and the
//!   countdown), `phone-qr-joined` (the same with a phone in: step 2 reads
//!   "Tersambung: Android"), `phone-qr-expired` (dimmed, with "Buat QR
//!   baru"), `phone-public` (the QR view on a Public network: "publik" on
//!   the network line and the warning in the hint box), `phone-nolan` (the
//!   warning card) and `phone-receive` (two files in, one at 62 %, one that
//!   failed). The `shelf` scene has two cards tagged "dari HP". The real
//!   flow in a browser on this PC: `--phone-debug` (see `phone/debug.rs`).
//! - `DI_LANG=en` draws the UI in English (also for `--demo-editor`).
//! - `DI_ACCENT=orange` and `DI_SURFACE=graphite` pick the island theme
//!   (`espresso`, `graphite`, `sea`, `night`, `plum`; without it, or `auto`,
//!   the surface follows the cat's colour as in the app).
//! - `DI_CAT_COLOR=calico` (`terracotta`, `calico`, `white`, `black`) and
//!   `DI_CAT_ACCESSORY=crown` (`none`, `glasses`, `bow`, `nightcap`, `scarf`,
//!   `flower`, `crown`) dress the cat in every scene, like `DI_NOTCH`; the
//!   scenes `bata-colors` (the four coats, each on the surface `auto` gives
//!   it, or on the one `DI_SURFACE` names) and `bata-accessories` (all seven
//!   accessories, five faces and the notch's 26 px, in the chosen coat, on
//!   the palette `DI_SURFACE` and `DI_ACCENT` pick) show them side by side
//!   (`--demo-robot`, or `DI_DEMO_STEP=bata-colors` in `--demo`).
//! - `DI_FONT=mono` sets the UI typeface to JetBrains Mono in every scene
//!   that builds a renderer (`--demo`, `--demo-cli`, `--demo-robot`,
//!   `--demo-devices`, `--demo-arrange`) and in `--settings`; unset or an
//!   unknown name is DM Sans (`UiFont::from_name`).
//! - `DI_CLOCK=0` draws the idle island without its clock.
//! - `DI_LYRIC_AT=ms` puts the lyric scenes at that position in the fake
//!   LRC (`DEMO_LRC`); `DI_DEMO_FADE=0.5` freezes the crossfade from the
//!   previous line at that progress.
//! - `DI_LYRIC_SCROLL=ms` freezes a line too long for the notch that long
//!   after it showed (held for the first `LYRIC_HOLD_MS`, then scrolling);
//!   without it the line scrolls live from the step's start.
//!
//! - `DI_NOTCH=WxH` sizes the closed island in every scene (`160x28` to
//!   `300x44`, clamped; 190x34 without it): the idle, music, timer and
//!   lyric views follow it, notices and the panel do not.
//! - `DI_NOTCH_FILL=transparent` draws the compact views and notices with the
//!   see-through background in every scene (the panel stays solid), at
//!   `DI_NOTCH_OPACITY=0..100` percent (30 by default); the scenes
//!   `idle-transparent` and `media-transparent` do it on their own.
//!
//! The lyric scenes size the notch as the island does: the line measured
//! by the renderer, capped by this screen's width (`lyric_max_w`).
//!
//! The copy preview's scenes (`clipboard-notice-text`, `-long`, `-image`,
//! `-files`, each also as `...-plain` without Bata) draw fixed made-up text,
//! a drawn thumbnail and three made-up file names; never the real clipboard.
//!
//! The cleaner's scenes (`cleaner`, `cleaner-confirm`, `cleaner-done`)
//! use fixed sizes and never start the scanner or the cleaner. Its Mole
//! scenes (`cleaner-mole`, `cleaner-mole-scanning`, `cleaner-mole-confirm`,
//! `cleaner-mole-log`, `cleaner-mole-result`, `cleaner-mole-unreadable`,
//! `cleaner-mole-missing`)
//! and the notices `mole-done` and `mole-failed` use the design's fixed
//! data; they never look for Mole, let alone run it.
//!
//! `--demo-devices` runs only the Devices page's scenes (`devices`,
//! `devices-detail`, `devices-ejecting`, `devices-empty`) and its notices
//! (`device-plugged`, `device-safe`), after `docs/design/rencana-v3.5.html`
//! p3: three made-up drives (SanDisk Ultra E:, WD Elements F: and G: refused
//! by Excel, Kartu SD H:), never a scan of this machine's drives and never an
//! eject. `DI_DEMO_STEP=devices` holds one in `--demo` as well.
//!
//! `dock-hover` and `dock-hover-edge` draw the page dots as the icon dock
//! (`ui::dock`), settled: the pointer over the 4th icon, or over the first.
//! `DI_DOCK_X=<dip>` puts the pointer at that x (window dip, the panel is
//! 560 wide) instead; without it the scenes show the settled scales.
//!
//! `--demo-robot` runs only Bata's scenes (`bata-*`): the sheet of faces and
//! sizes (studio-robot.html p2) and the notch states (p3). Their clocks are
//! fixed ("14:32", "23:48"), so nothing depends on the time of day. The
//! scenes `bata-look-left` and `bata-look-right` draw the idle cat looking at
//! a pointer at the far left and right of the screen (the largest turn: head
//! 7 dip, tilt 6 degrees, eyes 4 dip further). The scenes `bata-musik-joget`
//! (compact media, `Expr::Music`) and `bata-lirik-joget` (lyric view,
//! `Expr::Sing`) draw the cat dancing to the music, frozen at the deepest nod
//! (amount 1: the whole cat 1.5 dip down, 4 % wider, 6 % lower) with the note
//! half way up its rise and 30 % faded. Fixed values: the demos never capture
//! audio, and a real island nods with the bass or a 600 ms beat.
//!
//! The typing cat (work running: a Claude session, a CLI command, a Mole
//! clean) is `bata-cli-running`; `bata-ketik` (paws down, the eyes on the
//! screen), `bata-ketik-paw` (the left paw up), `bata-ketik-lirik-kiri` and
//! `bata-ketik-lirik-kanan` (the eyes glancing at a pointer far left and far
//! right, a little below) are fixed poses of it. All are the idle island
//! grown 12 dip on the left for the laptop (`idle_shape_typing`), the robot
//! and clock where the plain idle island has them; none reads the pointer or
//! runs the paw timer. The face sheet (`bata-sheet`) shows the typing cat
//! last, at the right of its cell so its laptop has room.
//!
//! The notch peek's scenes `peek-eyes` (robot on) and `peek-eyes-plain`
//! (robot off) draw the notch shrunk to the cat's two eyes: a 34 x 14 pill,
//! terracotta eyes, nothing else. Hold one with `DI_DEMO_STEP=peek-eyes` in
//! `--demo`. The eyes look and blink: `peek-eyes-left` and `peek-eyes-right`
//! look at a pointer at the zone's far left and right (the largest slide, 4
//! dip), `peek-eyes-down` at one low and to the right (1.5 dip down, 2 dip
//! across), `peek-eyes-blink` shows them shut (the cat's two level lines).
//! None of them reads the pointer.
//!
//! The busy-notch toast's scenes (`docs/memory/toast-while-busy.md`), made-up
//! data and a made-up logo only: `notice-busy-panel` (the Calendar page with
//! the toast's strip under its top edge), `notice-busy-lyric`,
//! `notice-busy-media` and `notice-busy-pomodoro` (the compact view grown by
//! the small logo's slot), `notice-busy-hover` (the full toast the pointer on
//! the logo peeks as, over a lyric context), `notice-unread-dot` (the
//! compact music view with the terracotta dot) and `notice-unread-dot-idle`
//! (the idle notch with it). A scene's morph from the one before plays as
//! in the island: the notch grows by the slot while the logo fades in.
//!
//! The notice ✕ (`docs/memory/notice-close.md`), made-up data only:
//! `notice-close` (the made-up chat toast with its logo, the ✕ shown),
//! `notice-close-hot` (the pointer on it), `notice-close-claude` (a Vale
//! notice with Bata), `notice-close-chip` (the Ready Pomodoro chip plus the ✕)
//! and `notice-close-reminder` (a sticky reminder: no ✕ even with the pointer
//! over it). `DI_DEMO_CLOSE=0.5` freezes the ✕'s fade at that alpha.
//!
//! `--demo-notice-close` runs the island window with a real `IslandState` fed
//! made-up notices: a chat toast with its logo, "Claude finished" with a made-up
//! session id, a finished clean and a sticky reminder, one after the other
//! (the queue refills when it is empty). The pointer on the island shows the ✕
//! with the same fade; a click applies `Event::Click(hit)` from the renderer's
//! hit test. Effects are never executed: they are printed to stderr
//! (`effects: [...]`), so a body click prints `OpenTarget` and a ✕ click prints
//! `[]`, and nothing is opened, focused or launched. No real clipboard,
//! notifications, media keys, config writes or history. `DI_DEMO_FULLSCREEN=1`
//! applies `Event::Fullscreen(true)` first (a closed notice then leaves the
//! island hidden); `DI_DEMO_STEP=notch:notice-close-live` is the Notch style.

use std::time::{Duration, Instant};

use chrono::{Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DispatchMessageW, MA_NOACTIVATE, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage,
    TranslateMessage, WM_DESTROY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_QUIT,
};

/// `WM_MOUSELEAVE` (the `windows` crate keeps it under Controls).
const WM_MOUSELEAVE: u32 = 0x02A3;

use super::cat_style::CatLook;
use super::dock::{self, DockFrame};
use super::geometry::{
    ANIM_MS, NOTCH_OPACITY_DEFAULT, NotchSize, ShapeKind, ShapeRect, TOAST_SLOT_EXTRA, Under,
    ease_out_back, idle_shape_typing, lerp_shape, lyric_box_w, lyric_max_w, lyric_scroll_offset,
    lyric_shape_sized, lyric_slot_width, lyric_width_min, shape_with_robot_sized, typing_lead,
    with_toast_slot,
};
use super::motion::{
    CloseFade, PAGE_FADE_MS, REVEAL_MS, RevealDir, Transit, close_fade_ms, content_alpha,
    page_fade_frame, reveal_frame,
};
use super::notch_fill::{self, fill_alpha};
use super::render::{
    CleanerView, CliView, ClipNoticeView, ClipView, DevicesView, FolderMenuView, Frame, MoleView,
    NoticeCloseView, NotifView, Progress, Renderer, RobotFrame, SearchView, SheetKind, ShotView,
    SystemView, ToastBadgeView, ToastLogoView,
};
use super::robot::{Expr, EyesPose, Motion, PAW_LIFT, Pose};
use super::shelf_motion::{SHELF_ENTER_MS, SHELF_MOVE_MS, SHELF_STAGGER_MS, ShelfMotion};
use super::window::IslandWindow;
use crate::calendar::{CalEvent, CalStatus, CalendarSnapshot, EventTime};
use crate::clean::{self, AdminOutcome, CatSize, CleanCat, CleanMode, CleanReport, ScanResult};
use crate::cli::{self, Output, Shell};
use crate::cli_folders::{FolderList, ListKind};
use crate::cli_panel::{Echoes, RunState};
use crate::clipboard::image::Thumb;
use crate::clipboard::store::ClipImage;
use crate::clipboard::{Clip, ClipData, ClipKind};
use crate::config::{NotchFill, Style, UiFont};
use crate::devices::{self, Bus, Device, DeviceKind, Partition, Veto, VetoKind};
use crate::i18n::{Key, tr};
use crate::lyrics::{self, FADE, LyricsFound, LyricsState, Query};
use crate::model::{CalView, Event, Hit, MediaInfo, Notice, NoticeKind, Page, ToastOrigin, View};
use crate::mole::{CleanRun, GroupSize, MoleOpts, MoleReport, PreviewSummary};
use crate::notes::Note;
use crate::notif::{NotifEntry, logo_key};
use crate::pomodoro::{PomoSettings, Pomodoro};
use crate::reminders::{Reminder, Repeat};
use crate::state::{IslandState, MolePhase};
use crate::system::{Alert, Drive, GIB, ProcUsage, SampleKind, SystemSample, alert_notice};
use crate::theme::{self, Accent, CatAccessory, CatColor, Palette, Surface};
use crate::tiles::{CustomKind, CustomTile, ShortcutTiles};

const PHASE: Duration = Duration::from_millis(2000);
/// The design's track position, 1:42 of 3:58 (about 43 %).
const DEMO_PROGRESS: Progress = Progress {
    position_ms: 102_000,
    end_ms: 238_000,
};

#[derive(Clone, Copy, PartialEq)]
enum Pomo {
    Idle,
    Running,
    Paused,
    Ready,
    /// A short break, running.
    Break,
}

/// What the phone link shows over the shelf in a scene (made-up data, no
/// network).
#[derive(Clone, Copy, PartialEq)]
enum PhoneDemo {
    No,
    /// The QR code of a made-up link, with its clock.
    Qr,
    /// The same, run out: dimmed, with "Buat QR baru".
    QrExpired,
    /// The same with a phone in: step 2 says who is connected.
    QrJoined,
    /// A Public network: the code with the network warning.
    Public,
    NoLan,
    /// Two files in, one at 62 %, one that failed.
    Receive,
}

/// The cat sheets a scene can draw instead of the island.
#[derive(Clone, Copy, PartialEq)]
enum Sheet {
    No,
    Faces,
    Colors,
    Accessories,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    kind: ShapeKind,
    view: View0,
    pomo: Pomo,
    cal_view: CalView,
    status: CalStatus,
    day_page: usize,
    notes: usize,
    /// Sample reminders on the reminder page: 5 or 12.
    reminders: usize,
    strip: bool,
    /// Position in `DEMO_LRC` for the lyric view.
    lyric_at: u64,
    /// "Lirik di notch" is on (the Musik page's button).
    lyrics_on: bool,
    /// The volume plate the wheel brings up, at this level.
    volume: Option<f32>,
    /// The Pintasan page with custom tiles among the built-in ones.
    custom_tiles: bool,
    /// The shelf with files being dragged over the island (its drop zone lit).
    shelf_drag: bool,
    /// The phone link's view over the shelf.
    phone: PhoneDemo,
    /// The quick search is open with the demo's made-up apps (and, in the
    /// `search-files` scene, made-up files and folders).
    search: bool,
    /// What the CLI page's card shows.
    cli: CliDemo,
    /// The track with very long names instead of the design's.
    long_media: bool,
    /// Bata's face; `None` draws the island without the robot.
    robot: Option<Expr>,
    /// A fixed idle clock instead of the time now.
    clock: Option<&'static str>,
    /// One of the cat sheets instead of the island.
    sheet: Sheet,
    /// The System page with drive C: under the 10 GB warning (ochre).
    system_low: bool,
    /// The cleaner over the System page, and how its button stands.
    clean: Option<CleanDemo>,
    /// What the Clipboard page shows.
    clip: ClipDemo,
    /// What the Notifikasi page shows.
    notif: NotifDemo,
    /// The page content is a sub-view `Reveal` in this direction.
    reveal: Option<RevealDir>,
    /// The page cross-fades in over this one (`PageFade`), as the search
    /// opens over a page.
    fade_from: Option<Page>,
    /// What the Devices page shows.
    devices: DevicesDemo,
    /// What the Susun page shows (and which overlay is on the screen).
    arrange: ArrangeDemo,
    /// The page dots as the dock, the pointer over this icon (0-based).
    dock: Option<usize>,
    /// The compact view or notice has the see-through background
    /// (`DI_NOTCH_OPACITY`, 30 % by default).
    transparent: bool,
    /// Where the idle cat looks, -1 (left edge of the screen) to 1 (right).
    look: f32,
    /// Where the shrunk notch's eyes look: across (-1 to 1) and down (0 to 1).
    eyes_look: (f32, f32),
    /// The shrunk notch's eyes are shut.
    eyes_blink: bool,
    /// The music cat at the deepest nod (amount 1) with its note half way up
    /// and a little faded: a fixed pose, nothing is captured.
    dance: bool,
    /// Where the typing cat's eyes glance: across (-1 to 1) and down (-1 to 1).
    glance: (f32, f32),
    /// How high the typing cat's left and right paw are raised, in grid units.
    paws: [f32; 2],
    /// A busy toast's small logo sits beside the compact view (the slot).
    slot: bool,
    /// The strip under the panel's top edge holds a busy toast, not the
    /// sticky Claude notice (with `strip`).
    toast_strip: bool,
    /// The strip holds the sticky update offer with its two chips (with
    /// `strip`).
    offer_strip: bool,
    /// The unread dot on the closed notch.
    dot: bool,
    /// The notice's ✕ is drawn (at `DI_DEMO_CLOSE`'s alpha, else fully); the
    /// flag says the pointer is on it. A notice that is not closable draws none.
    close: Option<bool>,
}

/// The cleaner's states (deepclean-opsi-d.html p10 and d1).
#[derive(Clone, Copy, PartialEq)]
enum CleanDemo {
    /// The top three ticked, the button showing the total.
    Ticks,
    /// The button armed: "Yakin? Hapus 2,3 GB".
    Armed,
    /// Mole mode, its preview done: 1,8 / 0,4 / 0,1 / 0,1 GB, ± 2,4 GB.
    MolePreview,
    /// Mole mode while the dry-run runs.
    MoleScanning,
    /// Mole mode armed: "Yakin? Hapus permanen".
    MoleArmed,
    /// Mole cleaning: the design's log lines.
    MoleLog,
    /// Mole done: the measured result over its log, "Pratinjau lagi".
    MoleResult,
    /// Mole's preview with one group it could not read ("lihat log").
    MoleUnreadable,
    /// Mole not installed: the "Cara memasang" card.
    MoleMissing,
}

/// The Clipboard page's states, after keputusan-v3.1.html §5. Fixed data
/// and a fixed "now": never the real clipboard, never the clock.
#[derive(Clone, Copy, PartialEq)]
enum ClipDemo {
    /// The design: six cards of 48, the first pinned and selected.
    Design,
    /// "denah" typed in the focused search field.
    Search,
    /// Nothing copied yet.
    Empty,
    /// "Riwayat clipboard" off.
    Off,
    /// A long history scrolled down a row: the scrollbar's thumb shows.
    Long,
}

/// The Notifikasi page's states. Fixed fake toasts and a fixed "now": never
/// the Action Center, never the listener, never the clock.
#[derive(Clone, Copy, PartialEq)]
enum NotifDemo {
    /// Eight rows, mixed read and unread, the times of every kind.
    Design,
    /// A row hovered: it is lit and shows its three buttons.
    Selected,
    /// Forty rows scrolled down five: the scrollbar's thumb shows.
    Long,
    /// Nothing caught yet.
    Empty,
    /// The listener has no access: the message and "Buka pengaturan".
    NoAccess,
    /// "Jeda" is on.
    Paused,
    /// "Hapus semua" waits for its second click ("Yakin?").
    Armed,
}

/// The CLI page's states, after pengingat-cli.html p3, p4 and p5.
#[derive(Clone, Copy, PartialEq)]
enum CliDemo {
    /// Nothing run yet, the input not clicked.
    Empty,
    /// p3 "Siap mengetik": two commands done, the input focused.
    Ready,
    /// p4 "Riwayat & saran": "git" typed, three suggestions.
    Typing,
    /// p3 "Perintah berjalan": npm run build at 0:42.
    Running,
    /// A command that ended with exit 0.
    Done,
    /// p4 "Selesai dengan error": npm test, exit 1 after 3.8 s.
    Error,
    /// p5: the enlarged panel with a long git log.
    Tall,
    /// keputusan-v3.1 p2: the page followed Explorer after a git status.
    FromExplorer,
    /// keputusan-v3.1 p2: the folder menu open (2 windows, 1 favourite,
    /// 2 recent).
    FolderMenu,
}

/// `View` without its payloads, so scenes can be `const`.
#[derive(Clone, Copy, PartialEq)]
enum View0 {
    Idle,
    MediaCompact,
    PomodoroCompact,
    Panel(Page),
    Notice(DemoNotice),
    MediaLyric,
}

/// One notice of each badge style.
#[derive(Clone, Copy, PartialEq)]
enum DemoNotice {
    /// A due reminder: accent badge, "Selesai".
    Reminder,
    /// A finished focus phase: badge and "Mulai" in the break's sage.
    Pomodoro,
    Claude,
    /// "Claude finished": a timed notice with a made-up session id.
    ClaudeDone,
    Battery,
    /// An app's notification: the letter badge.
    App,
    /// A real Windows toast whose app has a logo: the logo and the app's name.
    AppLogo,
    /// A browser toast with the site Windows gave.
    AppSite,
    /// A real Windows toast whose logo is unknown: the letter badge and the
    /// app's name.
    AppNoLogo,
    /// The morning greeting.
    Greeting,
    /// Claude at work (the design's "Claude Code" state).
    ClaudeWorking,
    /// Disk C: low (keputusan-v3.1.html p4): Bata low, "Buka".
    System,
    /// The cleaner finished (deepclean-opsi-d.html p10 "Selesai").
    CleanDone,
    /// Mole finished: 2,4 GB, measured on C: (d2 "Notch selama mode Mole").
    MoleDone,
    /// Mole failed with exit 1.
    MoleFailed,
    /// "Disalin" after copying the first Clipboard card.
    Copied,
    /// The copy preview (rencana-v3.5 request 3): two lines of copied text.
    ClipText,
    /// A very long single line with tabs: one line, fitted to the box.
    ClipLong,
    /// A copied picture: its thumbnail in the badge spot, "1280 × 720".
    ClipImage,
    /// Three copied files.
    ClipFiles,
    /// The screenshot preview (keputusan-v3.1 p6): 1280 × 720 · 245 KB.
    Shot,
    /// "Disimpan · Pictures\Screenshots" after Simpan.
    ShotSaved,
    /// "Dihapus · clipboard dikosongkan" after Hapus.
    ShotDeleted,
    /// A drive plugged in: SanDisk Ultra, E:, 38,9 GB free (rencana-v3.5 p3).
    DevicePlugged,
    /// "Aman dicabut" after Eject: SanDisk Ultra.
    DeviceSafe,
    /// A finished arrangement: 3 windows, with Batalkan and Simpan.
    Arranged,
    /// The self-update's four notices, with made-up versions (no network).
    UpdateAvailable,
    UpdateProgress,
    UpdateFailed,
    UpdateDone,
}

/// The history entry the demo's copy previews name (`DemoNotice::Clip*`).
const DEMO_CLIP_PREVIEW: u64 = 1;
/// The copied text of `clipboard-notice-text`: two lines.
const DEMO_COPY_TWO_LINES: &str =
    "Denah lantai 2 sudah saya revisi\r\nTolong dicek bagian tangga dan void ya";
/// And of `clipboard-notice-long`: one line far wider than the notch, with
/// tabs between the words.
const DEMO_COPY_LONG: &str = "Rekap\tbiaya\tpekerjaan\tstruktur\tlantai\t2\tdan\t3\t\
\tRumah\tKemang\tRaya\tNo.\t12\t\tdikerjakan\toleh\tPT\tBangun\tCipta\tSejahtera\t\
bersama\tkonsultan\tstruktur\tdan\tMEP\tuntuk\tkeperluan\ttender\tbulan\tdepan";

/// The screenshot preview's id in the demo (`DemoNotice::Shot`).
const DEMO_SHOT: u64 = 1;
/// Its fixed figures: the design's 1280 × 720, a 245 KB file.
const DEMO_SHOT_SIZE: (u32, u32, u64) = (1280, 720, 250_880);

/// The preview's picture, drawn like the design's `.shot` (a dark title
/// bar, a cream page, an orange heading and striped lines) at 460 × 259,
/// the 1280 × 720 screenshot fitted to the 2× box. Never a real capture.
fn demo_shot_thumb() -> Thumb {
    let (w, h) = (460u32, 259u32);
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    let bar_h = (h as f32 * 0.18) as u32;
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32, y as f32);
            let rgb: [u8; 3] = if y < bar_h {
                [0x2B, 0x22, 0x1C]
            } else if (27.0..=203.0).contains(&fx) && (68.0..=88.0).contains(&fy) {
                [0xC9, 0x54, 0x2B]
            } else if fx >= 27.0
                && fx <= w as f32 - 36.0
                && fy >= 108.0
                && fy <= h as f32 - 26.0
                && ((fy - 108.0) as u32 / 15) % 2 == 0
            {
                [0xE6, 0xDC, 0xCF]
            } else {
                [0xF4, 0xEE, 0xE6]
            };
            bgra.extend([rgb[2], rgb[1], rgb[0], 255]);
        }
    }
    Thumb { w, h, bgra }
}

const BASE: Scene = Scene {
    name: "",
    kind: ShapeKind::Panel,
    view: View0::Idle,
    pomo: Pomo::Idle,
    cal_view: CalView::Month,
    status: CalStatus::Ok,
    day_page: 0,
    notes: 4,
    reminders: 5,
    strip: false,
    lyric_at: 2_000,
    lyrics_on: false,
    volume: None,
    custom_tiles: false,
    shelf_drag: false,
    phone: PhoneDemo::No,
    search: false,
    cli: CliDemo::Empty,
    long_media: false,
    robot: None,
    clock: None,
    sheet: Sheet::No,
    system_low: false,
    clean: None,
    clip: ClipDemo::Design,
    notif: NotifDemo::Design,
    reveal: None,
    fade_from: None,
    devices: DevicesDemo::Design,
    arrange: ArrangeDemo::No,
    dock: None,
    transparent: false,
    look: 0.0,
    eyes_look: (0.0, 0.0),
    eyes_blink: false,
    dance: false,
    glance: (0.0, 0.0),
    paws: [0.0, 0.0],
    slot: false,
    toast_strip: false,
    offer_strip: false,
    dot: false,
    close: None,
};

const fn clipboard(name: &'static str, which: ClipDemo) -> Scene {
    Scene {
        name,
        view: View0::Panel(Page::Clipboard),
        clip: which,
        ..BASE
    }
}

/// A scene of Bata in the notch (studio-robot.html p3).
const fn bata(name: &'static str, kind: ShapeKind, view: View0, expr: Expr) -> Scene {
    Scene {
        name,
        kind,
        view,
        robot: Some(expr),
        clock: Some("14:32"),
        ..BASE
    }
}

/// Made-up lyrics: a metadata tag, an offset, a line timed twice, an
/// instrumental gap, a line too long for the notch and a short one.
const DEMO_LRC: &str = "[ar:Demo]\n[ti:Jalan Pulang]\n[offset:+0]\n\
[00:01.00][00:24.00] Lampu kota menyala satu per satu\n\
[00:05.20] Kita pulang lewat jalan yang biasa\n\
[00:10.00]\n\
[00:16.00] Langit sore memeluk atap-atap rumah di sepanjang jalan menuju pantai yang jauh di sana\n\
[00:26.00] Tanpa kata\n";

const fn lyric(name: &'static str, lyric_at: u64) -> Scene {
    Scene {
        name,
        kind: ShapeKind::MediaLyric,
        view: View0::MediaLyric,
        lyric_at,
        lyrics_on: true,
        ..BASE
    }
}

const fn panel(name: &'static str, page: Page) -> Scene {
    Scene {
        name,
        view: View0::Panel(page),
        ..BASE
    }
}

const fn notice(name: &'static str, which: DemoNotice) -> Scene {
    Scene {
        name,
        kind: ShapeKind::Notice,
        view: View0::Notice(which),
        ..BASE
    }
}

const fn cli(name: &'static str, which: CliDemo) -> Scene {
    Scene {
        name,
        view: View0::Panel(Page::Cli),
        kind: if matches!(which, CliDemo::Tall) {
            ShapeKind::PanelTall
        } else {
            ShapeKind::Panel
        },
        cli: which,
        ..BASE
    }
}

/// The cleaner over the System page.
const fn cleaner(name: &'static str, which: CleanDemo) -> Scene {
    Scene {
        name,
        kind: ShapeKind::PanelClean,
        view: View0::Panel(Page::System),
        clean: Some(which),
        ..BASE
    }
}

/// The Devices page's states (rencana-v3.5.html p3): fixed, made-up drives
/// and never a real scan or eject.
#[derive(Clone, Copy, PartialEq)]
enum DevicesDemo {
    /// The design's three drives, the WD Elements refused by Excel.
    Design,
    /// The WD Elements expanded: model, connection, file system, partitions.
    Detail,
    /// The SanDisk is being ejected: its button reads "Melepas…".
    Ejecting,
    /// Nothing plugged in.
    Empty,
}

/// What the Devices page draws in a scene: the drives, the keys being
/// ejected, the refusals and the key of the expanded card.
type DevicesState = (
    Vec<Device>,
    std::collections::HashSet<String>,
    std::collections::HashMap<String, Veto>,
    Option<String>,
);

/// The design's three drives: a SanDisk Ultra E: (exFAT, USB 3, 18,4 of
/// 57,3 GB), a WD Elements F: and G: (two NTFS partitions, 612 of 931 GB)
/// and a Kartu SD H: (FAT32, 3,1 of 29,7 GB).
fn demo_devices_list() -> Vec<Device> {
    let gb = |x: f64| (x * GIB as f64) as u64;
    let part = |letter: char, label: &str, fs: &str, total: f64, used: f64| Partition {
        letter,
        label: label.into(),
        fs: fs.into(),
        total: gb(total),
        free: Some(gb(total) - gb(used)),
    };
    vec![
        Device {
            key: "USB\\VID_0781&PID_5581\\DEMO-A".into(),
            name: "SanDisk Ultra".into(),
            model: "SanDisk Ultra".into(),
            kind: DeviceKind::Usb,
            bus: Bus::Usb3,
            partitions: vec![part('E', "SANDISK", "exFAT", 57.3, 18.4)],
        },
        Device {
            key: "USB\\VID_1058&PID_2621\\DEMO-B".into(),
            name: "WD Elements".into(),
            model: "WD Elements 2621".into(),
            kind: DeviceKind::Hdd,
            bus: Bus::Usb3,
            partitions: vec![
                part('F', "Proyek", "NTFS", 500.0, 350.0),
                part('G', "Arsip", "NTFS", 431.0, 262.0),
            ],
        },
        Device {
            key: "USB\\VID_058F&PID_6366\\DEMO-C".into(),
            name: "Kartu SD".into(),
            model: "Generic Card Reader".into(),
            kind: DeviceKind::Sd,
            bus: Bus::CardReader,
            partitions: vec![part('H', "", "FAT32", 29.7, 3.1)],
        },
    ]
}

fn demo_devices(which: DevicesDemo) -> DevicesState {
    let mut list = demo_devices_list();
    let (wd, sandisk) = (list[1].key.clone(), list[0].key.clone());
    let mut ejecting = std::collections::HashSet::new();
    let mut vetoes = std::collections::HashMap::new();
    let mut expanded = None;
    match which {
        DevicesDemo::Design => {
            vetoes.insert(
                wd,
                Veto {
                    kind: VetoKind::WindowsApp,
                    name: "Excel (Laporan.xlsx)".into(),
                },
            );
        }
        DevicesDemo::Detail => expanded = Some(wd),
        DevicesDemo::Ejecting => {
            ejecting.insert(sandisk);
        }
        DevicesDemo::Empty => list.clear(),
    }
    (list, ejecting, vetoes, expanded)
}

const fn notifications(name: &'static str, which: NotifDemo) -> Scene {
    Scene {
        name,
        view: View0::Panel(Page::Notifications),
        notif: which,
        ..BASE
    }
}

const fn devices_page(name: &'static str, which: DevicesDemo) -> Scene {
    Scene {
        name,
        view: View0::Panel(Page::Devices),
        devices: which,
        ..BASE
    }
}

const fn arrange_page(name: &'static str, which: ArrangeDemo) -> Scene {
    Scene {
        name,
        view: View0::Panel(Page::Arrange),
        arrange: which,
        ..BASE
    }
}

const SCENES: [Scene; 167] = [
    Scene {
        name: "idle",
        kind: ShapeKind::Idle,
        ..BASE
    },
    // The see-through background (Settings -> Tampilan -> Notch).
    Scene {
        name: "idle-transparent",
        kind: ShapeKind::Idle,
        transparent: true,
        ..BASE
    },
    Scene {
        name: "pomo-running",
        kind: ShapeKind::PomodoroCompact,
        view: View0::PomodoroCompact,
        pomo: Pomo::Running,
        ..BASE
    },
    Scene {
        name: "pomo-paused",
        kind: ShapeKind::PomodoroCompact,
        view: View0::PomodoroCompact,
        pomo: Pomo::Paused,
        ..BASE
    },
    Scene {
        name: "pomo-ready",
        kind: ShapeKind::PomodoroCompact,
        view: View0::PomodoroCompact,
        pomo: Pomo::Ready,
        ..BASE
    },
    Scene {
        name: "media-compact",
        kind: ShapeKind::MediaCompact,
        view: View0::MediaCompact,
        ..BASE
    },
    Scene {
        name: "media-transparent",
        kind: ShapeKind::MediaCompact,
        view: View0::MediaCompact,
        transparent: true,
        ..BASE
    },
    // The wheel over the compact notch: the volume plate at 64 %.
    Scene {
        name: "media-volume",
        kind: ShapeKind::MediaCompact,
        view: View0::MediaCompact,
        volume: Some(0.64),
        ..BASE
    },
    Scene {
        volume: Some(0.64),
        ..panel("music-volume", Page::Media)
    },
    panel("cal-month", Page::Calendar),
    Scene {
        cal_view: CalView::Week,
        ..panel("cal-week", Page::Calendar)
    },
    Scene {
        cal_view: CalView::Day,
        ..panel("cal-day", Page::Calendar)
    },
    Scene {
        status: CalStatus::NeedsLogin,
        ..panel("cal-needslogin", Page::Calendar)
    },
    Scene {
        cal_view: CalView::Day,
        status: CalStatus::Offline,
        day_page: 1,
        ..panel("cal-day-offline-p1", Page::Calendar)
    },
    Scene {
        pomo: Pomo::Running,
        ..panel("pomodoro", Page::Pomodoro)
    },
    Scene {
        pomo: Pomo::Break,
        ..panel("pomodoro-break", Page::Pomodoro)
    },
    panel("reminders-5", Page::Reminders),
    Scene {
        reminders: 12,
        ..panel("reminders-12", Page::Reminders)
    },
    notice("reminder-notice", DemoNotice::Reminder),
    // Focus done: the short break waits, so badge and chip are sage.
    Scene {
        pomo: Pomo::Ready,
        ..notice("notice-pomodoro", DemoNotice::Pomodoro)
    },
    notice("notice-claude", DemoNotice::Claude),
    notice("notice-battery", DemoNotice::Battery),
    notice("notice-system", DemoNotice::System),
    notice("notice-app", DemoNotice::App),
    notice("notice-app-logo", DemoNotice::AppLogo),
    notice("notice-app-site", DemoNotice::AppSite),
    notice("notice-app-no-logo", DemoNotice::AppNoLogo),
    // A real toast over a busy notch (`notification-history`'s toast, tucked).
    Scene {
        strip: true,
        toast_strip: true,
        ..panel("notice-busy-panel", Page::Calendar)
    },
    Scene {
        slot: true,
        ..lyric("notice-busy-lyric", 6_000)
    },
    Scene {
        name: "notice-busy-media",
        kind: ShapeKind::MediaCompact,
        view: View0::MediaCompact,
        slot: true,
        ..BASE
    },
    Scene {
        name: "notice-busy-pomodoro",
        kind: ShapeKind::PomodoroCompact,
        view: View0::PomodoroCompact,
        pomo: Pomo::Running,
        slot: true,
        ..BASE
    },
    // The pointer on the logo: the full toast, over a lyric context.
    Scene {
        lyric_at: 6_000,
        lyrics_on: true,
        ..notice("notice-busy-hover", DemoNotice::AppLogo)
    },
    Scene {
        name: "notice-unread-dot",
        kind: ShapeKind::MediaCompact,
        view: View0::MediaCompact,
        dot: true,
        ..BASE
    },
    Scene {
        name: "notice-unread-dot-idle",
        kind: ShapeKind::Idle,
        dot: true,
        robot: Some(Expr::Idle),
        clock: Some("14:32"),
        ..BASE
    },
    // The ✕ of a timed full notice: shown, lit under the pointer, on a Vale
    // notice with Bata, beside a chip, and absent on a sticky reminder.
    Scene {
        close: Some(false),
        ..notice("notice-close", DemoNotice::AppLogo)
    },
    Scene {
        close: Some(true),
        ..notice("notice-close-hot", DemoNotice::AppLogo)
    },
    Scene {
        robot: Some(Expr::Happy),
        close: Some(false),
        ..notice("notice-close-claude", DemoNotice::ClaudeDone)
    },
    Scene {
        pomo: Pomo::Ready,
        close: Some(false),
        ..notice("notice-close-chip", DemoNotice::Pomodoro)
    },
    Scene {
        close: Some(true),
        ..notice("notice-close-reminder", DemoNotice::Reminder)
    },
    panel("notes-4", Page::Notes),
    Scene {
        notes: 6,
        ..panel("notes-6", Page::Notes)
    },
    Scene {
        notes: 30,
        ..panel("notes-30", Page::Notes)
    },
    panel("media", Page::Media),
    Scene {
        long_media: true,
        ..panel("media-long", Page::Media)
    },
    Scene {
        strip: true,
        ..panel("strip", Page::Calendar)
    },
    // A medium line hugged by the notch, one capped and scrolling, the
    // instrumental rest and a short line (both the minimum).
    lyric("lyric", 6_000),
    lyric("lyric-long", 17_000),
    lyric("lyric-instrumental", 11_000),
    lyric("lyric-short", 27_000),
    Scene {
        lyrics_on: true,
        ..panel("media-lyrics-on", Page::Media)
    },
    // Fixed figures (keputusan-v3.1.html p4); never the real machine.
    Scene {
        custom_tiles: true,
        ..panel("shortcuts-custom", Page::Shortcuts)
    },
    panel("mixer", Page::Mixer),
    panel("shelf", Page::Shelf),
    panel("shelf-empty", Page::Shelf),
    Scene {
        shelf_drag: true,
        ..panel("shelf-drop", Page::Shelf)
    },
    Scene {
        shelf_drag: true,
        ..panel("shelf-drop-files", Page::Shelf)
    },
    // The cards' motion (`DI_DEMO_SHELF` freezes it).
    panel("shelf-enter", Page::Shelf),
    panel("shelf-exit", Page::Shelf),
    panel("shelf-move", Page::Shelf),
    // The phone link over the shelf, with made-up data and no network.
    Scene {
        phone: PhoneDemo::Qr,
        ..panel("phone-qr", Page::Shelf)
    },
    Scene {
        phone: PhoneDemo::QrJoined,
        ..panel("phone-qr-joined", Page::Shelf)
    },
    Scene {
        phone: PhoneDemo::QrExpired,
        ..panel("phone-qr-expired", Page::Shelf)
    },
    Scene {
        phone: PhoneDemo::Public,
        ..panel("phone-public", Page::Shelf)
    },
    Scene {
        phone: PhoneDemo::NoLan,
        ..panel("phone-nolan", Page::Shelf)
    },
    Scene {
        phone: PhoneDemo::Receive,
        ..panel("phone-receive", Page::Shelf)
    },
    Scene {
        search: true,
        ..panel("search", Page::Search)
    },
    // Files and folders among the apps, with icons, a folder's count and an
    // app's size filled in (made-up names and drawn icons, never the real
    // folders).
    Scene {
        search: true,
        ..panel("search-files", Page::Search)
    },
    panel("system", Page::System),
    Scene {
        system_low: true,
        ..panel("system-low", Page::System)
    },
    // Three made-up drives (rencana-v3.5 p3); never a real scan or eject.
    devices_page("devices", DevicesDemo::Design),
    devices_page("devices-detail", DevicesDemo::Detail),
    devices_page("devices-ejecting", DevicesDemo::Ejecting),
    devices_page("devices-empty", DevicesDemo::Empty),
    notice("device-plugged", DemoNotice::DevicePlugged),
    notice("device-safe", DemoNotice::DeviceSafe),
    // Susun: made-up windows and monitors; the overlays are click-through.
    arrange_page("arrange-templates", ArrangeDemo::Templates),
    arrange_page("arrange-hover", ArrangeDemo::Hover),
    arrange_page("arrange-pick", ArrangeDemo::Pick),
    notice("arrange-done", DemoNotice::Arranged),
    // Self-update notices with a made-up 9.9.9; nothing touches the network.
    // The sticky offer: the notch with "Nanti" and "Update"; over an open
    // panel the same offer is the strip, both chips drawn.
    notice("update-available", DemoNotice::UpdateAvailable),
    Scene {
        strip: true,
        offer_strip: true,
        ..panel("update-available-strip", Page::Calendar)
    },
    notice("update-progress", DemoNotice::UpdateProgress),
    notice("update-failed", DemoNotice::UpdateFailed),
    notice("update-done", DemoNotice::UpdateDone),
    // The page dots hovered: icons, magnified around the pointer.
    Scene {
        dock: Some(3),
        ..panel("dock-hover", Page::Calendar)
    },
    Scene {
        dock: Some(0),
        ..panel("dock-hover-edge", Page::Calendar)
    },
    // Fixed sizes (deepclean-opsi-d.html); never a real scan.
    cleaner("cleaner", CleanDemo::Ticks),
    cleaner("cleaner-confirm", CleanDemo::Armed),
    Scene {
        robot: Some(Expr::Happy),
        ..notice("cleaner-done", DemoNotice::CleanDone)
    },
    // Mole mode with the design's data; Mole is never looked for or run.
    cleaner("cleaner-mole", CleanDemo::MolePreview),
    cleaner("cleaner-mole-scanning", CleanDemo::MoleScanning),
    cleaner("cleaner-mole-confirm", CleanDemo::MoleArmed),
    cleaner("cleaner-mole-log", CleanDemo::MoleLog),
    cleaner("cleaner-mole-result", CleanDemo::MoleResult),
    cleaner("cleaner-mole-unreadable", CleanDemo::MoleUnreadable),
    cleaner("cleaner-mole-missing", CleanDemo::MoleMissing),
    Scene {
        robot: Some(Expr::Happy),
        ..notice("mole-done", DemoNotice::MoleDone)
    },
    Scene {
        robot: Some(Expr::Sad),
        ..notice("mole-failed", DemoNotice::MoleFailed)
    },
    cli("cli-empty", CliDemo::Empty),
    cli("cli-ready", CliDemo::Ready),
    cli("cli-typing", CliDemo::Typing),
    cli("cli-running", CliDemo::Running),
    cli("cli-done", CliDemo::Done),
    cli("cli-error", CliDemo::Error),
    cli("cli-tall", CliDemo::Tall),
    cli("cli-from-explorer", CliDemo::FromExplorer),
    cli("cli-folder-menu", CliDemo::FolderMenu),
    panel("shortcuts", Page::Shortcuts),
    clipboard("clipboard", ClipDemo::Design),
    clipboard("clipboard-search", ClipDemo::Search),
    clipboard("clipboard-empty", ClipDemo::Empty),
    clipboard("clipboard-off", ClipDemo::Off),
    clipboard("clipboard-long", ClipDemo::Long),
    // The Notifikasi page: fixed fake toasts, one with a drawn logo.
    notifications("notifications", NotifDemo::Design),
    notifications("notifications-selected", NotifDemo::Selected),
    notifications("notifications-long", NotifDemo::Long),
    notifications("notifications-empty", NotifDemo::Empty),
    notifications("notifications-no-access", NotifDemo::NoAccess),
    notifications("notifications-paused", NotifDemo::Paused),
    notifications("notifications-armed", NotifDemo::Armed),
    // keputusan-v3.1 §5 "Saat menyalin": Bata happy over the preview.
    bata(
        "clipboard-copied",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::Copied),
        Expr::Happy,
    ),
    // The copy preview (rencana-v3.5 request 3), with Bata happy and with
    // the badge; the picture takes the badge spot either way, never Bata.
    // Fixed text, a drawn thumbnail: never the real clipboard.
    bata(
        "clipboard-notice-text",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::ClipText),
        Expr::Happy,
    ),
    notice("clipboard-notice-text-plain", DemoNotice::ClipText),
    bata(
        "clipboard-notice-long",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::ClipLong),
        Expr::Happy,
    ),
    notice("clipboard-notice-long-plain", DemoNotice::ClipLong),
    bata(
        "clipboard-notice-image",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::ClipImage),
        Expr::Happy,
    ),
    notice("clipboard-notice-image-plain", DemoNotice::ClipImage),
    bata(
        "clipboard-notice-files",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::ClipFiles),
        Expr::Happy,
    ),
    notice("clipboard-notice-files-plain", DemoNotice::ClipFiles),
    // keputusan-v3.1 p6: the preview with Bata happy, then "Disimpan"
    // (happy) and "Dihapus" (at rest). Fixed figures and a drawn picture.
    bata(
        "screenshot",
        ShapeKind::NoticeShot,
        View0::Notice(DemoNotice::Shot),
        Expr::Happy,
    ),
    bata(
        "screenshot-saved",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::ShotSaved),
        Expr::Happy,
    ),
    bata(
        "screenshot-deleted",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::ShotDeleted),
        Expr::Idle,
    ),
    Scene {
        name: "bata-sheet",
        sheet: Sheet::Faces,
        ..BASE
    },
    Scene {
        name: "bata-colors",
        sheet: Sheet::Colors,
        ..BASE
    },
    Scene {
        name: "bata-accessories",
        sheet: Sheet::Accessories,
        ..BASE
    },
    bata("bata-kosong", ShapeKind::Idle, View0::Idle, Expr::Idle),
    Scene {
        clock: Some("23:48"),
        ..bata("bata-tidur", ShapeKind::Idle, View0::Idle, Expr::Sleep)
    },
    bata(
        "bata-sapaan",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::Greeting),
        Expr::Wave,
    ),
    Scene {
        pomo: Pomo::Running,
        ..bata(
            "bata-pomodoro",
            ShapeKind::PomodoroCompact,
            View0::PomodoroCompact,
            Expr::Focus,
        )
    },
    bata(
        "bata-musik",
        ShapeKind::MediaCompact,
        View0::MediaCompact,
        Expr::Music,
    ),
    Scene {
        lyric_at: 6_000,
        lyrics_on: true,
        ..bata(
            "bata-lirik",
            ShapeKind::MediaLyric,
            View0::MediaLyric,
            Expr::Sing,
        )
    },
    Scene {
        lyric_at: 17_000,
        lyrics_on: true,
        ..bata(
            "bata-lirik-long",
            ShapeKind::MediaLyric,
            View0::MediaLyric,
            Expr::Sing,
        )
    },
    // The cat dancing to the music: the deepest nod, the note half way up
    // and a little faded (fixed values, no audio capture).
    Scene {
        dance: true,
        ..bata(
            "bata-musik-joget",
            ShapeKind::MediaCompact,
            View0::MediaCompact,
            Expr::Music,
        )
    },
    Scene {
        dance: true,
        lyric_at: 6_000,
        lyrics_on: true,
        ..bata(
            "bata-lirik-joget",
            ShapeKind::MediaLyric,
            View0::MediaLyric,
            Expr::Sing,
        )
    },
    bata(
        "bata-pengingat",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::Reminder),
        Expr::Alert,
    ),
    Scene {
        pomo: Pomo::Ready,
        ..bata(
            "bata-fase",
            ShapeKind::Notice,
            View0::Notice(DemoNotice::Pomodoro),
            Expr::Happy,
        )
    },
    bata(
        "bata-claude",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::ClaudeWorking),
        Expr::Think,
    ),
    // Other apps keep their badge: no robot.
    Scene {
        robot: None,
        ..bata(
            "bata-notifikasi",
            ShapeKind::Notice,
            View0::Notice(DemoNotice::App),
            Expr::Idle,
        )
    },
    bata(
        "bata-cli-running",
        ShapeKind::Idle,
        View0::Idle,
        Expr::Typing,
    ),
    // The typing cat: paws down and the eyes on the screen, the left paw up,
    // and the eyes glancing at a pointer far left / right (a little below).
    // The island is grown on the left for the laptop.
    bata("bata-ketik", ShapeKind::Idle, View0::Idle, Expr::Typing),
    Scene {
        paws: [PAW_LIFT, 0.0],
        ..bata("bata-ketik-paw", ShapeKind::Idle, View0::Idle, Expr::Typing)
    },
    Scene {
        glance: (-1.0, 0.3),
        ..bata(
            "bata-ketik-lirik-kiri",
            ShapeKind::Idle,
            View0::Idle,
            Expr::Typing,
        )
    },
    Scene {
        glance: (1.0, 0.3),
        ..bata(
            "bata-ketik-lirik-kanan",
            ShapeKind::Idle,
            View0::Idle,
            Expr::Typing,
        )
    },
    bata("bata-cli-done", ShapeKind::Idle, View0::Idle, Expr::Happy),
    bata("bata-cli-failed", ShapeKind::Idle, View0::Idle, Expr::Sad),
    bata(
        "bata-sistem",
        ShapeKind::Notice,
        View0::Notice(DemoNotice::System),
        Expr::Sad,
    ),
    // The idle cat looking at a pointer at the far left / right of the screen.
    Scene {
        look: -1.0,
        ..bata("bata-look-left", ShapeKind::Idle, View0::Idle, Expr::Idle)
    },
    Scene {
        look: 1.0,
        ..bata("bata-look-right", ShapeKind::Idle, View0::Idle, Expr::Idle)
    },
    // The notch shrunk to the cat's two eyes (`pointer_watch::Peek`): the
    // pill alone, with the robot on and off.
    bata("peek-eyes", ShapeKind::Eyes, View0::Idle, Expr::Idle),
    Scene {
        robot: None,
        ..bata("peek-eyes-plain", ShapeKind::Eyes, View0::Idle, Expr::Idle)
    },
    // The eyes looking at a pointer, and shut (a fixed pose, no pointer read).
    Scene {
        eyes_look: (-1.0, 0.0),
        ..bata("peek-eyes-left", ShapeKind::Eyes, View0::Idle, Expr::Idle)
    },
    Scene {
        eyes_look: (1.0, 0.0),
        ..bata("peek-eyes-right", ShapeKind::Eyes, View0::Idle, Expr::Idle)
    },
    Scene {
        eyes_look: (0.5, 1.0),
        ..bata("peek-eyes-down", ShapeKind::Eyes, View0::Idle, Expr::Idle)
    },
    Scene {
        eyes_blink: true,
        ..bata("peek-eyes-blink", ShapeKind::Eyes, View0::Idle, Expr::Idle)
    },
    // Sub-view reveals (`DI_DEMO_REVEAL`): the new content fades in from
    // 8 px away. The week became the day; the cleaner just opened. The
    // search came over the Music page: the two pages cross-fade.
    Scene {
        cal_view: CalView::Day,
        reveal: Some(RevealDir::Open),
        ..panel("cal-week-to-day", Page::Calendar)
    },
    Scene {
        reveal: Some(RevealDir::Open),
        ..cleaner("cleaner-open", CleanDemo::Ticks)
    },
    Scene {
        search: true,
        fade_from: Some(Page::Media),
        ..panel("search-open", Page::Search)
    },
];

/// The design's CPU line: 60 points (60 s, one per second), ending on the ring's 33 %.
const DEMO_CPU: [f32; 60] = [
    22.0, 24.0, 26.0, 28.0, 30.0, 28.0, 27.0, 26.0, 25.0, 29.0, 33.0, 37.0, 41.0, 40.0, 39.0, 38.0,
    38.0, 42.0, 45.0, 48.0, 52.0, 48.0, 44.0, 40.0, 35.0, 32.0, 30.0, 29.0, 28.0, 30.0, 31.0, 32.0,
    33.0, 36.0, 40.0, 44.0, 47.0, 44.0, 42.0, 36.0, 31.0, 30.0, 29.0, 31.0, 33.0, 34.0, 36.0, 34.0,
    31.0, 30.0, 28.0, 29.0, 30.0, 32.0, 35.0, 33.0, 31.0, 32.0, 33.0, 33.0,
];

/// The System page's fixed figures (keputusan-v3.1.html `sysPanel`): CPU
/// 33 % on 8 cores, RAM 14,2 of 31,8 GB (44 %), C: and D:, three apps,
/// up 6 h 1 m. `low`: C: has 9,6 GB left, under the 10 GB warning.
fn demo_system(low: bool) -> SystemSample {
    let gb = |x: f64| (x * GIB as f64) as u64;
    let app = |name: &str, cpu: f32, mem: f64| ProcUsage {
        name: name.into(),
        cpu,
        mem: gb(mem),
    };
    SystemSample {
        kind: SampleKind::Page,
        cpu: 33.0,
        cores: 8,
        // Shown as 14,2 GB, and 44 % rather than the 45 % 14.2 would round to.
        ram_used: gb(14.1505),
        ram_total: gb(31.8),
        drives: vec![
            Drive {
                letter: 'C',
                free: gb(if low { 9.6 } else { 47.2 }),
                total: gb(338.4),
            },
            Drive {
                letter: 'D',
                free: gb(33.0),
                total: gb(137.0),
            },
        ],
        procs: vec![
            app("Chrome", 12.0, 3.1),
            app("Code", 7.0, 1.4),
            app("AutoCAD", 5.0, 2.2),
        ],
        uptime: Duration::from_secs(6 * 3600 + 60),
    }
}

/// The cleaner's sizes in the design: 0,1 / 1,8 / 0,4 / 0,1 GB, the rest
/// not measured ("—").
fn demo_scan() -> ScanResult {
    let gb = |x: f64| CatSize {
        bytes: (x * GIB as f64) as u64,
        files: 1,
        measured: true,
    };
    let unknown = CatSize::default();
    ScanResult {
        cats: [
            gb(0.1),
            gb(1.8),
            gb(0.4),
            gb(0.1),
            unknown,
            unknown,
            unknown,
        ],
        skipped_browsers: Vec::new(),
    }
}

/// Mole's preview in the design (`MG`): 1,8 / 0,4 / 0,1 / 0,1 GB, ± 2,4 GB
/// in 29 entries; `unreadable` leaves the app caches as "lihat log".
fn demo_mole_summary(unreadable: bool) -> PreviewSummary {
    let gb = |x: f64| (x * GIB as f64) as u64;
    let apps = if unreadable {
        GroupSize::Unreadable
    } else {
        GroupSize::Bytes(gb(0.4))
    };
    PreviewSummary {
        groups: [
            GroupSize::Bytes(gb(1.8)),
            apps,
            GroupSize::Bytes(gb(0.1)),
            GroupSize::Bytes(gb(0.1)),
        ],
        total: Some((gb(if unreadable { 2.0 } else { 2.4 }), 29)),
    }
}

/// Mole's log while it cleans (`logLines` in deepclean-opsi-d.html).
fn demo_mole_log() -> std::collections::VecDeque<String> {
    [
        "Clean Your Windows",
        "Free space: 47,24 GB",
        "\u{25B8} User essentials",
        "  User temp files (34 items, 61 MB)",
        "\u{25B8} Browser caches",
        "  Chrome CacheStorage (\u{2026} 922 MB)",
        "  Chrome Code Cache (\u{2026} 507 MB)",
        "\u{25B8} Developer tools \u{2026}",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Mole finished: 2,4 GB freed, C: from 47,2 to 49,6 GB; `failed`: exit 1.
fn demo_mole_report(failed: bool) -> MoleReport {
    let gb = |x: f64| (x * GIB as f64) as u64;
    MoleReport {
        run: CleanRun {
            exit: Some(if failed { 1 } else { 0 }),
            admin: AdminOutcome::NotAsked,
        },
        freed: gb(if failed { 0.4 } else { 2.4 }),
        drive: Some(('C', gb(47.2), gb(49.6))),
    }
}

/// The finished clean: 2,3 GB freed, C: now at 49,5 GB, 3 files in use.
fn demo_clean_report() -> CleanReport {
    let gb = |x: f64| (x * GIB as f64) as u64;
    CleanReport {
        freed: gb(2.3),
        drive: Some(('C', gb(49.5))),
        skipped: 3,
        skipped_browsers: Vec::new(),
        admin: AdminOutcome::NotAsked,
    }
}

/// `DI_DEMO_STEP`: a step number, or a scene's name (Pill), or
/// `notch:` and a name.
fn held_step(scenes: &[Scene], value: &str) -> Option<usize> {
    if let Ok(n) = value.trim().parse() {
        return Some(n);
    }
    let (offset, name) = match value.trim().strip_prefix("notch:") {
        Some(name) => (scenes.len(), name),
        None => (0, value.trim()),
    };
    scenes
        .iter()
        .position(|s| s.name == name)
        .map(|i| i + offset)
}

fn env<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok()?.parse().ok()
}

pub fn run() {
    run_scenes(&SCENES);
}

/// `--demo-cli`: only the CLI page's scenes (`cli-empty`, `cli-ready`,
/// `cli-typing`, `cli-running`, `cli-done`, `cli-error`, `cli-tall`,
/// `cli-from-explorer`, `cli-folder-menu`), Pill
/// then Notch; `DI_DEMO_STEP` picks one by number or name as in `--demo`.
/// `--demo-robot`: only Bata's scenes, Pill then Notch; `DI_DEMO_STEP`
/// picks one by number or name as in `--demo`.
pub fn run_robot() {
    let scenes: Vec<Scene> = SCENES
        .iter()
        .copied()
        .filter(|s| s.name.starts_with("bata-"))
        .collect();
    run_scenes(&scenes);
}

/// `--demo-devices`: only the Devices page's scenes and notices, Pill then
/// Notch; `DI_DEMO_STEP` picks one by number or name as in `--demo`.
pub fn run_devices() {
    let scenes: Vec<Scene> = SCENES
        .iter()
        .copied()
        .filter(|s| s.name.starts_with("device"))
        .collect();
    run_scenes(&scenes);
}

/// What the Susun page shows in a demo scene (`--demo-arrange`).
#[derive(Clone, Copy, PartialEq)]
enum ArrangeDemo {
    /// Not a Susun scene.
    No,
    /// The template step: two fake monitors, the built-ins, a custom
    /// template and two saved arrangements.
    Templates,
    /// The same with a card under the pointer: its outline is on the screen
    /// through the click-through overlay.
    Hover,
    /// The pick step, tile 2 of 3 current and the first filled: the real
    /// overlay (click-through in a demo) is on the screen.
    Pick,
}

/// The made-up windows of the pick list: fixed titles, no real window.
const DEMO_WINDOWS: [&str; 5] = [
    "main.rs \u{2014} vale \u{2014} Visual Studio Code",
    "Proposal Renovasi.docx \u{2014} Word",
    "Inbox (3) \u{2014} Mail \u{2014} Chrome",
    "Denah Lantai 2.dwg \u{2014} AutoCAD",
    "Spotify",
];

fn demo_arrange_view(which: ArrangeDemo) -> super::render::ArrangeView<'static> {
    use super::render::{ArrangeCardView, ArrangeSavedView, ArrangeView, ArrangeWindowView};
    use crate::arrange::{BuiltIn, NRect};
    let lang = crate::i18n::lang();
    let mut cards: Vec<ArrangeCardView> = BuiltIn::ALL
        .iter()
        .map(|b| ArrangeCardView {
            name: crate::i18n::arrange_builtin_name(lang, *b).to_owned(),
            rects: b.node().to_rects(),
        })
        .collect();
    let mut custom = crate::arrange::Node::Leaf;
    custom.split(0, crate::arrange::Dir::Cols);
    custom.split(1, crate::arrange::Dir::Rows);
    cards.push(ArrangeCardView {
        name: "Gambar + 2".to_owned(),
        rects: custom.to_rects(),
    });
    let saved = |name: &str, node: &crate::arrange::Node, apps: &str| ArrangeSavedView {
        name: name.to_owned(),
        rects: node.to_rects(),
        apps: apps.to_owned(),
    };
    let picking = which == ArrangeDemo::Pick;
    // The first window is taken by tile 1: out of the list.
    let windows = if picking {
        DEMO_WINDOWS[1..]
            .iter()
            .map(|t| ArrangeWindowView {
                title: (*t).to_owned(),
                key: "",
                icon: None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let _ = NRect::FULL;
    ArrangeView {
        picking: picking.then_some((1, 3)),
        monitors: vec![
            crate::i18n::arrange_monitor_chip(1, 2560, 1440),
            crate::i18n::arrange_monitor_chip(2, 1920, 1080),
        ],
        monitor: 0,
        cards,
        saved: if picking {
            Vec::new()
        } else {
            vec![
                saved(
                    "Kerja",
                    &BuiltIn::Main2.node(),
                    "Code \u{b7} Chrome \u{b7} Terminal",
                ),
                saved("Rapat", &BuiltIn::Cols2.node(), "Teams \u{b7} OneNote"),
            ]
        },
        windows,
        scroll: 0.0,
        hover_card: (which == ArrangeDemo::Hover).then_some(2),
        name: None,
    }
}

/// The overlays of an Arrange scene on the real screen, click-through, with
/// made-up tiles: the pick overlay (tile 2 of 3 current, the first filled)
/// or the hover outline. Never a real window list, never the mover.
fn demo_arrange_overlays(
    which: ArrangeDemo,
    island: windows::Win32::Foundation::HWND,
    at: (i32, i32),
) {
    use crate::arrange::{BuiltIn, Rect};
    use crate::arrange_overlay::{self, PickSpec, TileState};
    let area = crate::capture::win::virtual_screen();
    // The monitor the island is on.
    let (monitor, dpi) = crate::capture::win::monitor_at((at.0 + 8, at.1 + 8));
    // The work area is close enough to the monitor for a demo.
    let work = Rect::new(monitor.x, monitor.y, monitor.w, monitor.h - 48);
    let tiles = crate::arrange::tile_rects(
        &BuiltIn::Main2.node().to_rects(),
        work,
        crate::arrange::scaled_gap(8, dpi),
    );
    let accent = theme::current().accent.rgb();
    match which {
        ArrangeDemo::Pick => {
            arrange_overlay::outline_hide();
            if !arrange_overlay::active() {
                let spec = PickSpec {
                    island,
                    tiles,
                    accent,
                    allowed: Vec::new(),
                    windows: Vec::new(),
                    area,
                    demo: true,
                };
                if arrange_overlay::start(spec) {
                    arrange_overlay::update(
                        vec![
                            TileState::Filled {
                                name: "Code".to_owned(),
                                key: String::new(),
                                icon: None,
                            },
                            TileState::Current,
                            TileState::Waiting,
                        ],
                        Vec::new(),
                    );
                }
            }
        }
        ArrangeDemo::Hover => {
            arrange_overlay::close();
            arrange_overlay::outline_show(island, work, &tiles, accent);
        }
        _ => {
            arrange_overlay::close();
            arrange_overlay::outline_hide();
        }
    }
}

/// `--demo-arrange`: only the Susun page's scenes, Pill then Notch;
/// `DI_DEMO_STEP` picks one by number or name as in `--demo`. The demo
/// never calls the mover and never lists or moves a real window.
pub fn run_arrange() {
    let scenes: Vec<Scene> = SCENES
        .iter()
        .copied()
        .filter(|s| s.name.starts_with("arrange"))
        .collect();
    run_scenes(&scenes);
}

pub fn run_cli() {
    let scenes: Vec<Scene> = SCENES
        .iter()
        .copied()
        .filter(|s| s.name.starts_with("cli-"))
        .collect();
    run_scenes(&scenes);
}

fn run_scenes(scenes: &[Scene]) {
    run_scenes_with(scenes, None);
}

/// The demo loop; `live` is `--demo-notice-close`'s state, which then decides
/// the view, the pointer and the ✕ (the one scene only gives the look).
fn run_scenes_with(scenes: &[Scene], mut live: Option<LiveNoticeClose>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    // `DI_NOTCH_MONITOR=N`: the display with this number (left to right, then
    // top to bottom); unset or not a display: the primary monitor.
    let displays = crate::arrange_win::displays();
    let target = std::env::var("DI_NOTCH_MONITOR")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .and_then(|n| displays.iter().find(|d| d.number == n));
    let proc = if live.is_some() {
        live_wnd_proc
    } else {
        wnd_proc
    };
    let mut win = IslandWindow::create(Some(proc), target).expect("create island window");
    // Room for the enlarged CLI panel in every scene.
    win.set_tall(true);
    let mut renderer = Renderer::new().expect("create renderer");
    // `DI_FONT=mono` (unset or unknown: Sans) applies to every scene.
    let ui_font = UiFont::from_name(&std::env::var("DI_FONT").unwrap_or_default())
        .unwrap_or_default();
    renderer.set_ui_font(ui_font).expect("set the UI font");
    let media = MediaInfo {
        title: "Senja di Kemang".into(),
        artist: "Studio Ruang".into(),
        playing: true,
        art: Some(test_art()),
        source_app: "Spotify.exe".into(),
        session_key: 0,
        can_seek: false,
    };
    let long_media = MediaInfo {
        title: "Sampai Jadi Debu (Live at Synchronize Festival 2024, Extended Version)".into(),
        artist: "Banda Neira feat. Hindia, Kunto Aji, Nadin Amizah & Friends".into(),
        ..media.clone()
    };
    let shot_thumb = demo_shot_thumb();
    // The copied picture of `clipboard-notice-image`: the clipboard demo's.
    let clip_thumb = demo_thumb();
    let today = Local::now().date_naive();
    let name = |var| std::env::var(var).unwrap_or_default();
    // `DI_CAT_COLOR`, `DI_CAT_ACCESSORY` and `DI_SURFACE` apply to every
    // scene; a surface left out (or `auto`) follows the cat, as in the app.
    let cat = CatLook {
        color: CatColor::from_name(&name("DI_CAT_COLOR")).unwrap_or_default(),
        accessory: CatAccessory::from_name(&name("DI_CAT_ACCESSORY")).unwrap_or_default(),
    };
    let chosen_surface = Surface::from_name(&name("DI_SURFACE")).unwrap_or_default();
    let palette = theme::island(
        Accent::from_name(&name("DI_ACCENT")).unwrap_or_default(),
        chosen_surface.resolve(cat.color, true),
    );
    let calendar = demo_calendar(today, &palette);
    let notes = demo_notes(today);
    let notes30 = many_notes(today);
    let reminders5 = demo_reminders(today);
    let reminders12 = many_reminders(today);
    let (system_ok, system_low) = (demo_system(false), demo_system(true));
    let scan = demo_scan();
    let (mole_summary, mole_unreadable) = (demo_mole_summary(false), demo_mole_summary(true));
    let mole_log = demo_mole_log();
    let mole_report = demo_mole_report(false);
    let no_log = std::collections::VecDeque::new();
    let clips = demo_clips();
    let strip = Notice {
        key: "demo-wait".into(),
        source: "Claude".into(),
        title: tr(Key::ClaudeWaiting).into(),
        body: "Vale".into(),
        kind: NoticeKind::ClaudeWaiting {
            session_id: "demo".into(),
        },
    };
    // The busy toast of the strip and the slot: a made-up Telegram message.
    // The sticky update offer of the `update-available-strip` scene (made-up
    // 9.9.9, touches no network).
    let offer = Notice {
        key: "demo-update".into(),
        source: "Vale".into(),
        title: crate::i18n::update_available(crate::i18n::lang(), "9.9.9"),
        body: crate::i18n::update_installed(crate::i18n::lang(), env!("CARGO_PKG_VERSION")),
        kind: NoticeKind::UpdateAvailable {
            version: "9.9.9".into(),
        },
    };
    let busy_toast = Notice {
        key: "demo-busy-toast".into(),
        source: "Telegram".into(),
        title: "Rani".into(),
        body: "Denah lantai 2 sudah saya kirim, cek ya".into(),
        kind: NoticeKind::App {
            app_id: Some(DEMO_LOGO_KEY.into()),
            toast: Some(ToastOrigin::default()),
        },
    };
    let hold: Option<usize> = std::env::var("DI_DEMO_STEP")
        .ok()
        .and_then(|v| held_step(scenes, &v));
    // The live ✕ demo has one scene and never cycles.
    let hold = if live.is_some() {
        hold.or(Some(0))
    } else {
        hold
    };
    let frozen_close: Option<f32> = env("DI_DEMO_CLOSE");
    let frozen_t: Option<f32> = env("DI_DEMO_T");
    let slide: f32 = env("DI_DEMO_SLIDE").unwrap_or(0.0);
    let clock_on = std::env::var("DI_CLOCK").map_or(true, |v| v != "0");
    let lyric_at: Option<u64> = env("DI_LYRIC_AT");
    let lyric_fade: Option<f32> = env("DI_DEMO_FADE");
    let lyric_scroll: Option<u64> = env("DI_LYRIC_SCROLL");
    let frozen_reveal: Option<f32> = env("DI_DEMO_REVEAL");
    let notch = demo_notch();
    let frozen_shelf: Option<f32> = env("DI_DEMO_SHELF");
    let dock_x: Option<f32> = env("DI_DOCK_X");
    let steps = 2 * scenes.len();
    println!("demo: 2 s per view, Pill then Notch ({steps} steps); kill the process to stop");
    let start = Instant::now();
    let mut settled = usize::MAX;
    loop {
        if !pump_messages() {
            return;
        }
        if let Some(l) = live.as_mut() {
            l.step(&renderer, Instant::now());
        }
        let elapsed = start.elapsed();
        let (step, into) = match hold {
            Some(n) => (n % steps, elapsed.as_millis()),
            None => (
                (elapsed.as_millis() / PHASE.as_millis()) as usize % steps,
                elapsed.as_millis() % PHASE.as_millis(),
            ),
        };
        let t = frozen_t.unwrap_or((into as f32 / ANIM_MS as f32).min(1.0));
        let (style, mut scene) = scene(scenes, step);
        // The live demo's island is whatever its state shows now.
        let live_view = live.as_ref().map(LiveNoticeClose::view);
        if let (Some(l), Some(view)) = (live.as_ref(), live_view.as_ref()) {
            scene.robot = l.robot();
            scene.kind = if matches!(view, View::Notice(_)) {
                ShapeKind::Notice
            } else {
                ShapeKind::Idle
            };
        }
        if scene.sheet != Sheet::No {
            if step != settled {
                println!("demo: step {step} {:?} {}", style, scene.name);
                settled = step;
                let kind = match scene.sheet {
                    Sheet::Colors => SheetKind::Colors(chosen_surface),
                    Sheet::Accessories => SheetKind::Accessories,
                    _ => SheetKind::Faces,
                };
                if let Err(error) = renderer.draw_robot_sheet(&win, palette, kind, cat) {
                    eprintln!("demo: draw failed: {error}");
                }
            }
            std::thread::sleep(Duration::from_millis(16));
            continue;
        }
        let now = Instant::now();
        let at = lyric_at.unwrap_or(scene.lyric_at);
        let lyric_state = demo_lyrics(at, lyric_fade, now);
        let to = scene_shape(&renderer, &win, style, notch, &scene, at);
        // How far a long line has scrolled: frozen, or live from the step's start.
        let scroll = match scene.view {
            View0::MediaLyric => {
                let text = lyric_state.shown(now).map_or("", |l| l.text);
                let slot_w = if scene.slot { TOAST_SLOT_EXTRA } else { 0.0 };
                let box_w = lyric_box_w(to.w - slot_w, scene.robot.is_some());
                renderer.lyric_text_width(text).map_or(0.0, |text_w| {
                    lyric_scroll_offset(text_w, box_w, lyric_scroll.unwrap_or(into as u64))
                })
            }
            _ => 0.0,
        };
        // The equalizer and a scrolling line animate; everything else redraws
        // only while morphing.
        let reveal_playing = (scene.reveal.is_some() || scene.fade_from.is_some())
            && frozen_reveal.is_none()
            && into < (ANIM_MS + REVEAL_MS.max(PAGE_FADE_MS)) as u128 + 50;
        let shelf_span = shelf_scene_ms(scene.name);
        let shelf_playing =
            frozen_shelf.is_none() && shelf_span.is_some_and(|ms| into < ms as u128 + 50);
        let animated = scene.view == View0::MediaCompact
            || (scene.view == View0::MediaLyric && lyric_scroll.is_none() && scroll > 0.0)
            || reveal_playing
            || shelf_playing
            || live.is_some();
        if step != settled || animated {
            if t >= 1.0 {
                if settled != step {
                    println!("demo: step {step} {:?} {}", style, scene.name);
                    // Susun's overlays (click-through, made-up tiles) show
                    // with the settled scene and go with it.
                    demo_arrange_overlays(scene.arrange, win.hwnd(), win.screen_origin());
                }
                settled = step;
            }
            let (prev_style, prev) = self::scene(scenes, (step + steps - 1) % steps);
            let prev_at = lyric_at.unwrap_or(prev.lyric_at);
            let from = scene_shape(&renderer, &win, prev_style, notch, &prev, prev_at);
            let shape = lerp_shape(from, to, ease_out_back(t));
            // The fill follows the morph as the island does (`App::fill_at`).
            let fill_alpha =
                notch_fill::morph_fill(scene_fill(&prev), scene_fill(&scene), ease_out_back(t));
            let pomodoro = demo_pomodoro(scene.pomo, now);
            let view = live_view.unwrap_or_else(|| match scene.view {
                View0::Idle => View::Idle,
                View0::MediaCompact => View::MediaCompact,
                View0::PomodoroCompact => View::PomodoroCompact,
                View0::Panel(page) => View::Panel(page),
                View0::Notice(which) => View::Notice(demo_notice(which, &renderer)),
                View0::MediaLyric => View::MediaLyric,
            });
            // A fullscreen app with no notice left: nothing is drawn.
            let opacity = if view == View::Hidden { 0.0 } else { 1.0 };
            let notice_close = match &live {
                Some(l) => l.close_view(now),
                None => scene.close.map(|hot| NoticeCloseView {
                    alpha: frozen_close.unwrap_or(1.0).clamp(0.0, 1.0),
                    hot,
                }),
            }
            .filter(|c| c.alpha > 0.0);
            let page = match scene.view {
                View0::Panel(page) => page,
                _ => Page::Calendar,
            };
            let snapshot = CalendarSnapshot {
                status: scene.status,
                ..calendar.clone()
            };
            let cli = demo_cli(scene.cli, now);
            let notes: &[Note] = match scene.notes {
                30 => &notes30,
                n => &notes[..n.min(notes.len())],
            };
            let system = SystemView {
                sample: Some(if scene.system_low {
                    &system_low
                } else {
                    &system_ok
                }),
                eased: None,
                history: &DEMO_CPU,
                disk_warn_gb: 10,
                ram_warn_percent: 90,
            };
            let cleaner = scene.clean.map(|which| {
                let opts = MoleOpts::default();
                let previewed = MolePhase::Previewed { opts, ok: true };
                let phase = match which {
                    CleanDemo::Ticks | CleanDemo::Armed => MolePhase::Unknown,
                    CleanDemo::MolePreview | CleanDemo::MoleArmed | CleanDemo::MoleUnreadable => {
                        previewed
                    }
                    CleanDemo::MoleScanning => MolePhase::Scanning(opts),
                    CleanDemo::MoleLog => MolePhase::Running,
                    CleanDemo::MoleResult => MolePhase::Finished { ok: true },
                    CleanDemo::MoleMissing => MolePhase::Missing,
                };
                let with_log = matches!(which, CleanDemo::MoleLog | CleanDemo::MoleResult);
                let mole_mode = phase != MolePhase::Unknown;
                CleanerView {
                    mode: if mole_mode {
                        CleanMode::Mole
                    } else {
                        CleanMode::Own
                    },
                    checked: CleanCat::ALL.map(CleanCat::checked_at_start),
                    armed: matches!(which, CleanDemo::Armed | CleanDemo::MoleArmed),
                    scan: Some(&scan),
                    scanning: false,
                    busy: false,
                    mole: MoleView {
                        phase,
                        opts,
                        show_log: false,
                        version: Some("1.30.0"),
                        tested: true,
                        summary: match which {
                            CleanDemo::MoleUnreadable => Some(&mole_unreadable),
                            _ => Some(&mole_summary),
                        },
                        log: if with_log { &mole_log } else { &no_log },
                        report: (which == CleanDemo::MoleResult).then_some(&mole_report),
                        scroll: 0,
                    },
                }
            });
            let (dev_list, dev_ejecting, dev_vetoes, dev_expanded) = demo_devices(scene.devices);
            let dev_scroll = dev_expanded.as_deref().map_or(0.0, |key| {
                let shape = super::geometry::shape_for(style, ShapeKind::Panel);
                let shapes = super::panel_layout::device_shapes(&dev_list, &dev_vetoes, Some(key));
                let at = dev_list.iter().position(|d| d.key == key).unwrap_or(0);
                super::panel_layout::devices_reveal(shape, scene.strip, &shapes, 0.0, at)
            });
            let mixer_rows = demo_mixer();
            let shelf_elapsed = shelf_span.map_or(0, |ms| match frozen_shelf {
                Some(p) => (p.clamp(0.0, 1.0) * ms as f32) as u64,
                None => into as u64,
            });
            let (shelf_cards, shelf_motion) =
                demo_shelf_motion(scene.name, style, scene.strip, now, shelf_elapsed)
                    .unwrap_or_else(|| {
                        (
                            demo_shelf(matches!(scene.name, "shelf" | "shelf-drop-files")),
                            ShelfMotion::default(),
                        )
                    });
            let demo_phone = DemoPhone::new(scene.phone);
            let phone_view = demo_phone.view();
            let with_files = scene.name == "search-files";
            let search_query = if with_files {
                DEMO_FILES_QUERY
            } else {
                DEMO_SEARCH_QUERY
            };
            let search_results = if scene.search {
                demo_search_results(search_query, with_files)
            } else {
                Vec::new()
            };
            let search_details = if with_files {
                demo_search_details()
            } else {
                std::collections::HashMap::new()
            };
            let notif_items = demo_notif_items(scene.notif);
            let notif_logos: std::collections::HashMap<String, Thumb> = [
                (DEMO_LOGO_KEY.to_owned(), demo_logo([0x25, 0xA2, 0x5A])),
                (DEMO_CHROME_KEY.to_owned(), demo_logo([0x3B, 0x76, 0xE8])),
            ]
            .into();
            // The same lookup the island makes for a toast notice.
            let toast_logo = match &view {
                View::Notice(Notice {
                    kind:
                        NoticeKind::App {
                            app_id,
                            toast: Some(_),
                        },
                    source,
                    ..
                }) => notif_logos
                    .get_key_value(&logo_key(app_id.as_deref(), source))
                    .map(|(key, thumb)| ToastLogoView { key, thumb }),
                _ => None,
            };
            let frame = Frame {
                view: &view,
                media: Some(if scene.long_media {
                    &long_media
                } else {
                    &media
                }),
                pomodoro: &pomodoro,
                now,
                today,
                page,
                strip: scene.strip.then_some(if scene.toast_strip {
                    &busy_toast
                } else if scene.offer_strip {
                    &offer
                } else {
                    &strip
                }),
                cal_view: scene.cal_view,
                cal_anchor: today,
                cal_selected: today,
                calendar: &snapshot,
                mixer: Some(&mixer_rows),
                shelf: &shelf_cards,
                shelf_drag: scene.shelf_drag,
                shelf_motion: &shelf_motion,
                phone: phone_view.as_ref(),
                phone_on: scene.phone != PhoneDemo::No,
                devices: DevicesView {
                    list: &dev_list,
                    ejecting: &dev_ejecting,
                    vetoes: &dev_vetoes,
                    expanded: dev_expanded.as_deref(),
                    scroll: dev_scroll,
                },
                notes,
                notes_page: 0,
                reminders: if scene.reminders == 12 {
                    &reminders12
                } else {
                    &reminders5
                },
                reminders_page: 0,
                day_page: scene.day_page,
                page_slide: slide,
                // The demo slides between neighbours in the default order.
                page_trail: {
                    let i = Page::ALL.iter().position(|p| *p == page).unwrap_or(0);
                    let from = if slide > 0.0 {
                        i.checked_sub(1)
                    } else if slide < 0.0 {
                        Some(i + 1).filter(|j| *j < Page::ALL.len())
                    } else {
                        None
                    };
                    let x = slide - slide.signum();
                    [
                        from.map(|j| (Page::ALL[j], x))
                            .filter(|(_, x)| x.abs() < 1.0),
                        None,
                    ]
                },
                page_from: scene.fade_from,
                pages: Page::ALL.to_vec(),
                dock: scene.dock.map_or_else(DockFrame::default, |icon| {
                    let layout = dock::resting_layout(to, Page::ALL.len());
                    let x = dock_x.unwrap_or_else(|| layout[icon].x + dock::DOCK_ICON / 2.0);
                    DockFrame {
                        e: 1.0,
                        scales: dock::targets(x, &layout),
                    }
                }),
                tiles: scene_tiles(&scene),
                palette,
                cat,
                fill_alpha,
                legibility: notch_fill::legibility(fill_alpha),
                clock: match scene.clock {
                    Some(fixed) => Some(fixed.to_owned()),
                    None => clock_on.then(|| Local::now().format("%H:%M").to_string()),
                },
                idle_lead: if scene.view == View0::Idle && scene.robot.is_some() {
                    // Against the plain idle shape, as in the app.
                    let plain = shape_with_robot_sized(style, notch, ShapeKind::Idle, true);
                    typing_lead(shape, plain)
                } else {
                    0.0
                },
                lyric: lyric_state.shown(now),
                lyric_scroll: scroll,
                lyric_prev_scroll: 0.0,
                lyrics_on: scene.lyrics_on,
                lyrics_card_open: false,
                volume: scene.volume,
                cli: cli.view(),
                clip: clip_view(&clips, scene.clip),
                notif: notif_view(&notif_items, &notif_logos, scene.notif),
                arrange: demo_arrange_view(scene.arrange),
                search: SearchView {
                    query: search_query,
                    selection: (search_query.len(), search_query.len()),
                    focused: scene.search,
                    results: &search_results,
                    selected: 0,
                    details: &search_details,
                    indexing: false,
                },
                progress: Some(DEMO_PROGRESS),
                robot: scene.robot.map(|expr| RobotFrame {
                    expr,
                    pose: {
                        let pose = Pose {
                            paw_lift: scene.paws,
                            ..Pose::default().looking(scene.look)
                        }
                        .glancing(scene.glance.0, scene.glance.1);
                        if scene.dance {
                            pose.dance(1.0, 0.5, 0.3)
                        } else {
                            pose
                        }
                    },
                }),
                eyes: scene.kind == ShapeKind::Eyes,
                eyes_pose: {
                    let (dx_dip, dy_dip) =
                        crate::pointer_watch::eyes_offset_dip(scene.eyes_look.0, scene.eyes_look.1);
                    EyesPose {
                        dx_dip,
                        dy_dip,
                        blink: scene.eyes_blink,
                    }
                },
                eq_heights: None,
                robot_fades: false,
                system: Some(&system),
                cleaner: cleaner.as_ref(),
                shot: (scene.view == View0::Notice(DemoNotice::Shot)).then(|| {
                    let (w, h, bytes) = DEMO_SHOT_SIZE;
                    ShotView {
                        id: DEMO_SHOT,
                        thumb: &shot_thumb,
                        w,
                        h,
                        bytes,
                    }
                }),
                clip_notice: (scene.view == View0::Notice(DemoNotice::ClipImage)).then_some(
                    ClipNoticeView {
                        id: DEMO_CLIP_PREVIEW,
                        thumb: &clip_thumb,
                    },
                ),
                toast_logo,
                // The slot grows with the morph: the animated width over the
                // one without the slot (a scene's slot and line are fixed, so
                // this matches `App`'s `SlotShare` here).
                toast_slot_w: if scene.slot {
                    (shape.w - (to.w - TOAST_SLOT_EXTRA)).clamp(0.0, TOAST_SLOT_EXTRA)
                } else {
                    0.0
                },
                toast_slot: scene.slot.then(|| ToastBadgeView {
                    source: &busy_toast.source,
                    logo: notif_logos
                        .get_key_value(&logo_key(Some(DEMO_LOGO_KEY), &busy_toast.source))
                        .map(|(key, thumb)| ToastLogoView { key, thumb }),
                }),
                toast_dot: scene.dot,
                strip_badge: (scene.strip && scene.toast_strip).then(|| ToastBadgeView {
                    source: &busy_toast.source,
                    logo: notif_logos
                        .get_key_value(&logo_key(Some(DEMO_LOGO_KEY), &busy_toast.source))
                        .map(|(key, thumb)| ToastLogoView { key, thumb }),
                }),
                strip_motion: None,
                notice_close,
                transit: {
                    let reveal_at =
                        frozen_reveal.unwrap_or((into as f32 / REVEAL_MS as f32).min(1.0));
                    let compact = !matches!(scene.view, View0::Panel(_));
                    let morphing = t < 1.0;
                    Transit {
                        reveal: scene.reveal.map(|dir| reveal_frame(dir, 0.0, reveal_at)),
                        page_fade: scene.fade_from.and_then(|_| {
                            let at = frozen_reveal
                                .unwrap_or((into as f32 / PAGE_FADE_MS as f32).min(1.0));
                            page_fade_frame(at)
                        }),
                        // The morph's content: a compact view or notice
                        // fades in, the panel before it fades out.
                        content_alpha: if compact && morphing {
                            content_alpha(t)
                        } else {
                            1.0
                        },
                        leaving: match prev.view {
                            View0::Panel(page) if compact && morphing => Some(page),
                            _ => None,
                        },
                        // Into a view: its height; from one: the view's.
                        fade_floor_h: if compact {
                            to.h
                        } else if matches!(prev.view, View0::Panel(_)) {
                            notch.h
                        } else {
                            from.h.max(notch.h)
                        },
                    }
                },
            };
            if let Err(error) = renderer.draw(&win, style, shape, &frame, opacity) {
                eprintln!("demo: draw failed: {error}");
            }
        }
        std::thread::sleep(Duration::from_millis(if animated { 33 } else { 16 }));
    }
}

/// The design's `CLIPS2`: kind, pinned, value. The image card's value is
/// unused (its meta comes from the size).
const DEMO_CLIPS: [(ClipKind, bool, &str); 6] = [
    (
        ClipKind::Text,
        true,
        "Denah lantai 2 sudah saya revisi, tolong dicek bagian tangga",
    ),
    (ClipKind::Link, false, "github.com/tw93/mole/tree/windows"),
    (ClipKind::Image, false, ""),
    (ClipKind::Code, false, "cargo test --locked -- --nocapture"),
    (ClipKind::Color, false, "#E2683C"),
    (
        ClipKind::Text,
        false,
        "Jl. Kemang Raya No. 12, Jakarta Selatan",
    ),
];
/// "Cari di 48 item…": the six cards are the newest of 48.
const DEMO_CLIP_TOTAL: usize = 48;

/// The fixed "now" the cards' ages count from (never the clock).
fn demo_clip_now() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 27)
        .and_then(|d| d.and_hms_opt(14, 32, 0))
        .expect("valid time")
}

/// The six cards, copied a few minutes apart before `demo_clip_now`; the
/// screenshot 2 minutes ago, its thumbnail the design's terracotta
/// gradient with a pale frame.
fn demo_clips() -> Vec<Clip> {
    let now = demo_clip_now();
    let unix = |mins: i64| {
        Local
            .from_local_datetime(&(now - chrono::Duration::minutes(mins)))
            .earliest()
            .map_or(0, |t| t.timestamp())
    };
    DEMO_CLIPS
        .iter()
        .enumerate()
        .map(|(i, &(kind, pinned, value))| {
            let data = match kind {
                ClipKind::Image => ClipData::Image(ClipImage {
                    width: 1280,
                    height: 720,
                    thumb: demo_thumb(),
                    fingerprint: 1,
                    screenshot: true,
                    full: None,
                }),
                _ => ClipData::Text(value.into()),
            };
            Clip {
                id: i as u64 + 1,
                kind,
                data,
                pinned,
                at: unix(i as i64 * 2),
            }
        })
        .collect()
}

/// The fixed "now" of the Notifikasi scenes (never the clock).
fn demo_notif_now() -> chrono::DateTime<Local> {
    Local
        .with_ymd_and_hms(2026, 10, 31, 14, 32, 0)
        .earliest()
        .expect("a time that exists")
}

/// The logo key of the one demo app that has a logo.
const DEMO_LOGO_KEY: &str = "5319.WhatsApp!App";

/// The logo key of the demo browser.
const DEMO_CHROME_KEY: &str = "Chrome";

/// A made-up 48 x 48 logo: a rounded square of colour `body` with a pale
/// ring, drawn here, premultiplied (it is opaque inside, clear at the
/// corners).
fn demo_logo(body: [u8; 3]) -> Thumb {
    let side = 48u32;
    let mut bgra = Vec::with_capacity((side * side * 4) as usize);
    let (r, k) = (11.0f32, side as f32);
    for y in 0..side {
        for x in 0..side {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distance outside the rounded square.
            let dx = (fx - k / 2.0).abs() - (k / 2.0 - r);
            let dy = (fy - k / 2.0).abs() - (k / 2.0 - r);
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r;
            let alpha = (0.5 - outside).clamp(0.0, 1.0);
            let ring = ((fx - k / 2.0).hypot(fy - k / 2.0) - 13.0).abs() < 2.0;
            let rgb: [f32; 3] = if ring {
                [0xF2 as f32, 0xF7 as f32, 0xF3 as f32]
            } else {
                body.map(f32::from)
            };
            bgra.extend([
                (rgb[2] * alpha) as u8,
                (rgb[1] * alpha) as u8,
                (rgb[0] * alpha) as u8,
                (alpha * 255.0) as u8,
            ]);
        }
    }
    Thumb {
        w: side,
        h: side,
        bgra,
    }
}

/// One made-up toast: source, AUMID, title, body, site, seconds ago, read.
type DemoNotif = (
    &'static str,
    Option<&'static str>,
    &'static str,
    &'static str,
    Option<&'static str>,
    i64,
    bool,
);

/// The made-up toasts of the Notifikasi scenes, newest first: `(source,
/// AUMID, title, body, site, seconds ago, read)`.
const DEMO_NOTIFS: [DemoNotif; 8] = [
    (
        "WhatsApp",
        Some(DEMO_LOGO_KEY),
        "Ibu",
        "Jangan lupa makan siang ya",
        None,
        20,
        false,
    ),
    (
        "Teams",
        None,
        "Rapat desain 15.00",
        "Dimulai 30 menit lagi, ruang Kemang",
        None,
        5 * 60,
        false,
    ),
    (
        "Google Chrome",
        Some(DEMO_CHROME_KEY),
        "Rani",
        "Foto denah sudah saya kirim",
        Some("web.telegram.org"),
        42 * 60,
        true,
    ),
    (
        "Outlook",
        None,
        "Pak Hendra",
        "Revisi RAB sudah saya kirim, mohon dicek bagian pondasi dan tangga",
        None,
        3 * 3600,
        false,
    ),
    (
        "WhatsApp",
        Some(DEMO_LOGO_KEY),
        "Tim Studio",
        "",
        None,
        6 * 3600,
        true,
    ),
    (
        "Teams",
        None,
        "Dina mengirim berkas",
        "gambar-kerja-struktur.dwg",
        None,
        26 * 3600,
        true,
    ),
    (
        "Chrome",
        None,
        "Pembaruan tersedia",
        "Mulai ulang Chrome untuk menyelesaikan pembaruan",
        None,
        3 * 86_400,
        true,
    ),
    (
        "Outlook",
        None,
        "Undangan: Tinjauan proyek",
        "Jumat, 14.00 · Ruang rapat besar",
        None,
        40 * 86_400,
        true,
    ),
];

/// The entries of a Notifikasi scene (`DEMO_NOTIFS`, ids counting down so the
/// newest has the highest); forty of them for `Long`, none for the empty
/// and no-access scenes.
fn demo_notif_items(which: NotifDemo) -> Vec<NotifEntry> {
    let now = demo_notif_now().timestamp();
    let count = match which {
        NotifDemo::Empty | NotifDemo::NoAccess => 0,
        NotifDemo::Long => 40,
        _ => DEMO_NOTIFS.len(),
    };
    (0..count)
        .map(|i| {
            let (source, app_id, title, body, site, ago, read) = DEMO_NOTIFS[i % DEMO_NOTIFS.len()];
            // The long list spreads its times out, newest first.
            let ago = if count > DEMO_NOTIFS.len() {
                ago + (i / DEMO_NOTIFS.len()) as i64 * 90_000
            } else {
                ago
            };
            NotifEntry {
                id: (count - i) as u64,
                at: now - ago,
                read,
                source: source.into(),
                app_id: app_id.map(str::to_owned),
                title: title.into(),
                body: body.into(),
                site: site.map(str::to_owned),
            }
        })
        .collect()
}

/// The Notifikasi page for `which` over `items` and `logos`.
fn notif_view<'a>(
    items: &'a [NotifEntry],
    logos: &'a std::collections::HashMap<String, Thumb>,
    which: NotifDemo,
) -> NotifView<'a> {
    NotifView {
        items,
        logos,
        selected: match which {
            // The third row from the top.
            NotifDemo::Selected => items.get(2).map(|e| e.id),
            NotifDemo::Long => items.get(7).map(|e| e.id),
            _ => None,
        },
        scroll: if which == NotifDemo::Long { 5 } else { 0 },
        cap: 50,
        record: true,
        paused: which == NotifDemo::Paused,
        no_access: which == NotifDemo::NoAccess,
        armed: which == NotifDemo::Armed,
        now: demo_notif_now(),
    }
}

/// `.clip.img .th`: a 160° gradient #E2683C → #5A2A1C, a pale inner frame.
fn demo_thumb() -> Thumb {
    let (w, h) = (184u32, 104u32);
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    let (a, b) = ([0xE2, 0x68, 0x3C], [0x5A, 0x2A, 0x1C]);
    let inset = 28.0;
    for y in 0..h {
        for x in 0..w {
            let t = ((x as f32 * 0.34 + y as f32 * 0.94) / (w as f32 * 0.34 + h as f32 * 0.94))
                .clamp(0.0, 1.0);
            let mut rgb = [0f32; 3];
            for c in 0..3 {
                rgb[c] = a[c] as f32 + (b[c] as f32 - a[c] as f32) * t;
            }
            let (fx, fy) = (x as f32, y as f32);
            let on_frame = |v: f32, lo: f32, hi: f32| (v - lo).abs() < 1.5 || (v - hi).abs() < 1.5;
            let inside_x = fx >= inset - 1.5 && fx <= w as f32 - inset + 1.5;
            let inside_y = fy >= inset - 1.5 && fy <= h as f32 - inset + 1.5;
            let frame = (on_frame(fx, inset, w as f32 - inset) && inside_y)
                || (on_frame(fy, inset, h as f32 - inset) && inside_x);
            if frame {
                for (c, pale) in rgb.iter_mut().zip([0xF4, 0xED, 0xE4]) {
                    *c += (pale as f32 - *c) * 0.55;
                }
            }
            bgra.extend([rgb[2] as u8, rgb[1] as u8, rgb[0] as u8, 255]);
        }
    }
    Thumb { w, h, bgra }
}

/// The Clipboard page for `which`, from `clips` (`demo_clips`).
fn clip_view(clips: &[Clip], which: ClipDemo) -> ClipView<'_> {
    let now = demo_clip_now();
    match which {
        ClipDemo::Design => ClipView {
            items: clips.iter().collect(),
            total: DEMO_CLIP_TOTAL,
            selected: clips.first().map(|c| c.id),
            ..ClipView::empty(now)
        },
        ClipDemo::Search => {
            let query = "denah";
            let end = query.encode_utf16().count();
            let items: Vec<&Clip> = clips
                .iter()
                .filter(|c| {
                    c.plain_text()
                        .is_some_and(|t| t.to_lowercase().contains(query))
                })
                .collect();
            ClipView {
                selected: items.first().map(|c| c.id),
                items,
                total: DEMO_CLIP_TOTAL,
                query,
                selection: (end, end),
                focused: true,
                ..ClipView::empty(now)
            }
        }
        // Fourteen cards (the six again, then some): the panel shows three
        // rows of two, so it scrolls; one row down.
        ClipDemo::Long => ClipView {
            items: clips.iter().cycle().take(14).collect(),
            total: DEMO_CLIP_TOTAL,
            selected: clips.get(1).map(|c| c.id),
            scroll: 1,
            ..ClipView::empty(now)
        },
        ClipDemo::Empty => ClipView::empty(now),
        ClipDemo::Off => ClipView {
            enabled: false,
            ..ClipView::empty(now)
        },
    }
}

/// One notice per badge style, with the design's texts; the cleaner's are
/// fitted to the notice by `renderer` as the island does.
fn demo_notice(which: DemoNotice, renderer: &Renderer) -> Notice {
    let fits = |kind: NoticeKind| move |s: &str| renderer.notice_text_fits(&kind, false, s);
    let fits_either =
        |kind: NoticeKind| move |title: bool, s: &str| renderer.notice_text_fits(&kind, title, s);
    let notice = |source: &str, title: &str, body: String, kind| Notice {
        key: "demo".into(),
        source: source.into(),
        title: title.into(),
        body,
        kind,
    };
    match which {
        DemoNotice::Reminder => notice(
            tr(Key::SourceReminder),
            "Kirim revisi denah ke klien",
            format!(
                "{} · {}",
                tr(Key::Missed),
                crate::i18n::time_label(
                    crate::i18n::lang(),
                    NaiveTime::from_hms_opt(16, 30, 0).expect("valid time"),
                ),
            ),
            NoticeKind::Reminder { id: "demo".into() },
        ),
        DemoNotice::Pomodoro => notice(
            tr(Key::Pomodoro),
            tr(Key::FocusDone),
            crate::i18n::focus_done_body(crate::i18n::lang(), 5),
            NoticeKind::Pomodoro,
        ),
        DemoNotice::Claude => notice(
            "Claude",
            tr(Key::ClaudeWaiting),
            format!("vale · {}", tr(Key::DemoClaudeAsks)),
            NoticeKind::ClaudeWaiting {
                session_id: "demo".into(),
            },
        ),
        DemoNotice::ClaudeDone => notice(
            "Claude",
            tr(Key::ClaudeDone),
            "vale \u{b7} tugas selesai".into(),
            NoticeKind::ClaudeDone {
                session_id: "demo-session".into(),
            },
        ),
        DemoNotice::Battery => notice(
            tr(Key::SourceBattery),
            tr(Key::BatteryLow),
            crate::i18n::battery_body(crate::i18n::lang(), 20),
            NoticeKind::BatteryLow,
        ),
        DemoNotice::App => notice(
            "Mail",
            "Rani · Mail",
            "Denah lantai 2 sudah saya kirim, cek ya".into(),
            NoticeKind::App {
                app_id: None,
                toast: None,
            },
        ),
        DemoNotice::AppLogo => notice(
            "Telegram",
            "Rani",
            "Denah lantai 2 sudah saya kirim, cek ya".into(),
            NoticeKind::App {
                app_id: Some(DEMO_LOGO_KEY.into()),
                toast: Some(ToastOrigin::default()),
            },
        ),
        DemoNotice::AppSite => notice(
            "Google Chrome",
            "Rani",
            "Foto denah sudah saya kirim".into(),
            NoticeKind::App {
                app_id: Some(DEMO_CHROME_KEY.into()),
                toast: Some(ToastOrigin {
                    site: Some("web.telegram.org".into()),
                }),
            },
        ),
        DemoNotice::AppNoLogo => notice(
            "Slack",
            "Rani",
            "Denah lantai 2 sudah saya kirim, cek ya".into(),
            NoticeKind::App {
                app_id: Some("com.squirrel.slack.slack".into()),
                toast: Some(ToastOrigin::default()),
            },
        ),
        DemoNotice::Greeting => {
            // DI_GREET_PERIOD: morning (default), midday, afternoon, evening, night.
            let period = std::env::var("DI_GREET_PERIOD")
                .ok()
                .and_then(|v| crate::greeting::DayPeriod::from_id(v.trim()))
                .unwrap_or(crate::greeting::DayPeriod::Morning);
            let lang = crate::i18n::lang();
            let counts = crate::greeting::GreetCounts {
                events: 3,
                reminders: 2,
            };
            notice(
                tr(Key::SourceVale),
                crate::i18n::greeting_title(lang, period),
                crate::i18n::greeting_body(lang, period, counts),
                NoticeKind::Greeting { period },
            )
        }
        DemoNotice::Copied => notice(
            tr(Key::ClipboardTitle),
            tr(Key::ClipCopied),
            crate::clipboard::store::preview(DEMO_CLIPS[0].2, 120),
            NoticeKind::Copied,
        ),
        DemoNotice::ClipText | DemoNotice::ClipLong => {
            let lang = crate::i18n::lang();
            let text = if which == DemoNotice::ClipText {
                DEMO_COPY_TWO_LINES
            } else {
                DEMO_COPY_LONG
            };
            let (title, second) = crate::clipboard::notice::clip_notice_lines(text);
            let body = second.unwrap_or_else(|| {
                crate::i18n::clip_text_meta(lang, ClipKind::Text, text.chars().count())
            });
            Notice {
                key: crate::model::CLIP_PREVIEW_KEY.into(),
                ..notice(
                    tr(Key::ClipboardTitle),
                    &title,
                    body,
                    NoticeKind::ClipPreview {
                        id: DEMO_CLIP_PREVIEW,
                    },
                )
            }
        }
        DemoNotice::ClipImage => Notice {
            key: crate::model::CLIP_IMAGE_KEY.into(),
            ..notice(
                tr(Key::ClipboardTitle),
                tr(Key::ClipPreviewImage),
                crate::i18n::clip_preview_size(1280, 720),
                NoticeKind::ClipPreview {
                    id: DEMO_CLIP_PREVIEW,
                },
            )
        },
        DemoNotice::ClipFiles => Notice {
            key: crate::model::CLIP_PREVIEW_KEY.into(),
            ..notice(
                tr(Key::ClipboardTitle),
                tr(Key::ClipPreviewFiles),
                crate::i18n::clip_files(crate::i18n::lang(), 3, "laporan.pdf"),
                NoticeKind::ClipPreview {
                    id: DEMO_CLIP_PREVIEW,
                },
            )
        },
        DemoNotice::Shot => notice(
            tr(Key::ShotTitle),
            tr(Key::ShotTitle),
            String::new(),
            NoticeKind::Screenshot {
                shot: DEMO_SHOT,
                secs: 8,
                delete_if_ignored: false,
            },
        ),
        DemoNotice::ShotSaved => notice(
            tr(Key::ShotTitle),
            tr(Key::ShotSaved),
            r"Pictures\Screenshots".into(),
            NoticeKind::ShotSaved,
        ),
        DemoNotice::ShotDeleted => {
            let lang = crate::i18n::lang();
            let (title, body) =
                crate::i18n::shot_deleted(lang, crate::screenshot::FileOutcome::Recycled, true);
            notice(tr(Key::ShotTitle), title, body, NoticeKind::ShotDeleted)
        }
        DemoNotice::DevicePlugged => {
            devices::plugged_notice(crate::i18n::lang(), &demo_devices_list()[0])
        }
        DemoNotice::Arranged => notice(
            tr(Key::PageArrange),
            &crate::i18n::arrange_done(crate::i18n::lang(), 3, None),
            crate::i18n::arrange_builtin_name(crate::i18n::lang(), crate::arrange::BuiltIn::Main2)
                .to_owned(),
            NoticeKind::Arranged {
                save: crate::model::ArrangeSave::Offer,
            },
        ),
        DemoNotice::UpdateAvailable => notice(
            "Vale",
            &crate::i18n::update_available(crate::i18n::lang(), "9.9.9"),
            crate::i18n::update_installed(crate::i18n::lang(), env!("CARGO_PKG_VERSION")),
            NoticeKind::UpdateAvailable {
                version: "9.9.9".into(),
            },
        ),
        DemoNotice::UpdateProgress => notice(
            "Vale",
            &crate::i18n::update_downloading(crate::i18n::lang(), 42),
            "Vale 9.9.9".into(),
            NoticeKind::UpdateProgress { pct: 42 },
        ),
        DemoNotice::UpdateFailed => {
            let lang = crate::i18n::lang();
            notice(
                "Vale",
                &crate::i18n::update_failed(
                    lang,
                    crate::update::UpdateError::ChecksumMismatch.reason(lang),
                ),
                "Vale 9.9.9".into(),
                NoticeKind::UpdateFailed,
            )
        }
        DemoNotice::UpdateDone => notice(
            "Vale",
            &crate::i18n::update_done(crate::i18n::lang(), "9.9.9"),
            "9.9.8 \u{2192} 9.9.9".into(),
            NoticeKind::UpdateInfo,
        ),
        DemoNotice::DeviceSafe => {
            devices::safe_notice(crate::i18n::lang(), &["SanDisk Ultra".to_owned()])
        }
        DemoNotice::ClaudeWorking => notice(
            "Claude",
            tr(Key::DemoClaudeWorking),
            format!("vale · {}", tr(Key::DemoFilesChanged)),
            NoticeKind::ClaudeDone {
                session_id: "demo".into(),
            },
        ),
        DemoNotice::System => alert_notice(
            crate::i18n::lang(),
            &Alert::Disk {
                letter: 'C',
                free: (9.6 * GIB as f64) as u64,
            },
        ),
        DemoNotice::CleanDone => clean::done_notice(
            crate::i18n::lang(),
            &demo_clean_report(),
            &fits(NoticeKind::CleanDone),
        ),
        DemoNotice::MoleDone => crate::mole::done_notice(
            crate::i18n::lang(),
            &demo_mole_report(false),
            &fits_either(NoticeKind::MoleDone),
        ),
        DemoNotice::MoleFailed => crate::mole::done_notice(
            crate::i18n::lang(),
            &demo_mole_report(true),
            &fits_either(NoticeKind::MoleFailed),
        ),
    }
}

/// `DEMO_LRC` through the island's own lyric state, on the line at `at` ms;
/// with `fade`, crossfading into it from the line before at that progress.
fn demo_lyrics(at: u64, fade: Option<f32>, now: Instant) -> LyricsState {
    let lines = lyrics::parse_lrc(DEMO_LRC);
    let query = Query {
        key: "demo".into(),
        artist: "Demo".into(),
        title: "Jalan Pulang".into(),
        duration_ms: 30_000,
        provisional: false,
    };
    let mut state = LyricsState::default();
    state.want(Some(&query), true);
    state.found(LyricsFound {
        key: "demo".into(),
        lines: Some(std::sync::Arc::new(lines.clone())),
        transient: false,
    });
    if let Some(f) = fade {
        let line = lyrics::current_line(&lines, at).map_or(0, |(i, _)| i);
        let before = line.checked_sub(1).map_or(0, |i| lines[i].at_ms);
        state.advance(before, now);
        let since = now
            .checked_sub(FADE.mul_f32(f.clamp(0.0, 1.0)))
            .unwrap_or(now);
        state.advance(at, since);
    } else {
        state.advance(at, now);
    }
    state
}

/// What one CLI scene shows, owned for the frame.
struct DemoCli {
    output: Output,
    echoes: Echoes,
    run: RunState,
    input: &'static str,
    focused: bool,
    suggestions: Vec<&'static str>,
    tall: bool,
    cwd: &'static str,
    from_explorer: bool,
    menu: Option<FolderMenuView>,
}

impl DemoCli {
    fn view(&self) -> CliView<'_> {
        let end = self.input.encode_utf16().count();
        CliView {
            output: &self.output,
            echoes: &self.echoes,
            scroll: 0,
            run: self.run,
            shell: Shell::PowerShell,
            cwd: self.cwd,
            input: self.input,
            selection: (end, end),
            focused: self.focused,
            suggestions: self.suggestions.clone(),
            selected: 0,
            tall: self.tall,
            from_explorer: self.from_explorer,
            menu: self.menu.as_ref(),
        }
    }
}

/// The design's terminals (`OUT_READY`, `OUT_RUN`, `OUT_ERR`, `OUT_TALL`),
/// with commands echoed the way the island echoes them.
fn demo_cli(which: CliDemo, now: Instant) -> DemoCli {
    let mut output = Output::default();
    let mut echoes = Echoes::default();
    let mut command = |output: &mut Output, cmd: &str, lines: &[&str]| {
        echoes.mark(output);
        output.push(cli::echo_line(cmd));
        for line in lines {
            output.push((*line).to_owned());
        }
    };
    let ready = |output: &mut Output, command: &mut dyn FnMut(&mut Output, &str, &[&str])| {
        command(
            output,
            "git status",
            &["On branch main · nothing to commit, working tree clean"],
        );
        command(
            output,
            "ls *.dwg",
            &["denah-lt1.dwg   denah-lt2.dwg   tampak-depan.dwg"],
        );
    };
    let secs = |s: f32| Duration::from_secs_f32(s);
    let mut run = RunState::Idle;
    match which {
        CliDemo::Empty => {}
        CliDemo::Ready | CliDemo::Typing => ready(&mut output, &mut command),
        CliDemo::Running => {
            command(
                &mut output,
                "npm run build",
                &[
                    "> rumah-kemang@1.4.0 build",
                    "> vite build",
                    "vite v6.0.7 building for production...",
                    "\u{2713} 1284 modules transformed.",
                    "rendering chunks (12/18)...",
                ],
            );
            run = RunState::Running {
                since: now.checked_sub(secs(42.3)).unwrap_or(now),
            };
        }
        CliDemo::Done => {
            ready(&mut output, &mut command);
            command(
                &mut output,
                "git pull",
                &["Already up to date.", &cli::exit_line(Some(0), false)],
            );
            run = RunState::Ended {
                code: Some(0),
                killed: false,
                took: secs(2.1),
            };
        }
        CliDemo::Error => {
            command(
                &mut output,
                "npm test",
                &[
                    "PASS src/denah.test.ts",
                    "FAIL src/rab.test.ts",
                    "  \u{25CF} hitungRAB \u{203A} total termasuk PPN",
                    "    Expected: 12.210.000 \u{00B7} Received: 11.000.000",
                    "Tests: 1 failed, 23 passed, 24 total",
                ],
            );
            run = RunState::Ended {
                code: Some(1),
                killed: false,
                took: secs(3.8),
            };
        }
        CliDemo::Tall => {
            let log: Vec<String> = [
                ("b83979b", "fix: shortcut programs by full path"),
                ("bc85b93", "fix: shortcuts keep the panel closed"),
                ("118800e", "docs: english readme for v2.2"),
                ("77d3f16", "feat: shortcuts page and cli"),
                ("4106ddd", "fix: lyrics wait for the new track"),
                ("9c1e2aa", "feat: synced lyrics in the notch"),
                ("5d0b7f1", "feat: reminders"),
                ("a2e44c9", "feat: idle clock"),
                ("e81f3b0", "feat: settings window"),
                ("0b9d2c6", "feat: pomodoro ring"),
                ("7f3a510", "feat: google calendar"),
                ("3c6e9d1", "init"),
            ]
            .iter()
            .map(|(hash, msg)| format!("{hash} {msg}"))
            .collect();
            let lines: Vec<&str> = log.iter().map(String::as_str).collect();
            ready(&mut output, &mut command);
            command(&mut output, "git log --oneline -12", &lines);
        }
        CliDemo::FromExplorer => {
            command(
                &mut output,
                "git status",
                &["On branch main \u{00B7} nothing to commit"],
            );
            output.push(crate::i18n::cli_moved_to(
                crate::i18n::lang(),
                r"D:\github\project-a",
            ));
        }
        CliDemo::FolderMenu => ready(&mut output, &mut command),
    }
    let menu = (which == CliDemo::FolderMenu).then(demo_folder_menu);
    DemoCli {
        output,
        echoes,
        run,
        input: if which == CliDemo::Typing { "git" } else { "" },
        focused: which != CliDemo::Empty,
        suggestions: if which == CliDemo::Typing {
            vec!["git status", "git pull", "git log --oneline -12"]
        } else {
            Vec::new()
        },
        tall: which == CliDemo::Tall,
        cwd: match which {
            CliDemo::FromExplorer | CliDemo::FolderMenu => r"D:\github\project-a",
            _ => "D:\\Proyek\\rumah-kemang",
        },
        from_explorer: which == CliDemo::FromExplorer,
        menu,
    }
}

/// The design's menu: two Explorer windows, one favourite, two recent
/// folders (2 minutes ago and yesterday), timed from a fixed "now" of
/// 27 October 2026, 14.32, so the screenshot never depends on the clock.
fn demo_folder_menu() -> FolderMenuView {
    let day = NaiveDate::from_ymd_opt(2026, 10, 27).expect("valid date");
    let at = |d: NaiveDate, h, m| d.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid time"));
    let now = at(day, 14, 32);
    let explorer = [
        std::path::PathBuf::from(r"D:\github\project-a"),
        std::path::PathBuf::from(r"D:\Proyek\rumah-kemang\gambar"),
    ];
    let mut favorites = FolderList::new(ListKind::Favorites);
    favorites.set_favorite(std::path::Path::new(r"D:\github\dynamic-island"), true);
    let mut recent = FolderList::new(ListKind::Recent);
    let yesterday = day.pred_opt().expect("valid date");
    recent.push_recent(
        std::path::Path::new(r"C:\Users\hartono"),
        stamp(at(yesterday, 17, 10)),
    );
    recent.push_recent(
        std::path::Path::new(r"D:\Proyek\rumah-kemang"),
        stamp(at(day, 14, 30)),
    );
    let sections = crate::cli_folders::menu_rows(&explorer, &favorites, &recent, now, &|_| true);
    FolderMenuView {
        items: crate::cli_folders::menu_items(&sections),
    }
}

/// The scene's shape; a lyric scene's notch fits its line at `lyric_at` as
/// the island fits it on entering the view (measured, capped by the screen).
fn scene_shape(
    renderer: &Renderer,
    win: &IslandWindow,
    style: Style,
    notch: NotchSize,
    scene: &Scene,
    lyric_at: u64,
) -> ShapeRect {
    let robot = scene.robot.is_some();
    // The typing cat's idle island is wider on the left, as in the app.
    if scene.kind == ShapeKind::Idle && scene.robot == Some(Expr::Typing) {
        return idle_shape_typing(style, notch, robot);
    }
    if scene.kind != ShapeKind::MediaLyric {
        let shape = shape_with_robot_sized(style, notch, scene.kind, robot);
        // A busy toast's logo beside a compact view widens it.
        return if scene.slot {
            with_toast_slot(shape)
        } else {
            shape
        };
    }
    let state = demo_lyrics(lyric_at, None, Instant::now());
    let text_w = state
        .shown(Instant::now())
        .and_then(|l| renderer.lyric_text_width(l.text));
    let max_w = lyric_max_w(win.screen_w()).max(notch.w);
    let w = lyric_width_min(text_w, robot, notch.w, max_w);
    let w = if scene.slot {
        lyric_slot_width(w, robot, notch.w, max_w)
    } else {
        w
    };
    lyric_shape_sized(style, notch, w)
}

/// `DI_NOTCH=WxH` (e.g. `160x28`, `300x44`), clamped to the limits; the
/// island's 190 x 34 without it.
fn demo_notch() -> NotchSize {
    let parsed = std::env::var("DI_NOTCH").ok().and_then(|v| {
        let (w, h) = v
            .trim()
            .to_ascii_lowercase()
            .split_once('x')
            .map(|(w, h)| (w.trim().to_owned(), h.trim().to_owned()))?;
        Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
    });
    parsed.map_or(NotchSize::DEFAULT, |(w, h)| NotchSize::from_px(w, h))
}

/// The surface fill's alpha a scene's view is drawn with: the open panel
/// always solid; the compact views and notices solid unless the scene is a
/// `transparent` one or `DI_NOTCH_FILL=transparent` is set, then
/// `DI_NOTCH_OPACITY` percent (30 by default).
fn scene_fill(scene: &Scene) -> f32 {
    if matches!(scene.view, View0::Panel(_)) {
        return 1.0;
    }
    let env_transparent =
        std::env::var("DI_NOTCH_FILL").is_ok_and(|v| v.trim().eq_ignore_ascii_case("transparent"));
    let fill = if scene.transparent || env_transparent {
        NotchFill::Transparent
    } else {
        NotchFill::Solid
    };
    let pct = env::<u8>("DI_NOTCH_OPACITY").unwrap_or(NOTCH_OPACITY_DEFAULT);
    fill_alpha(fill, pct.min(100))
}

fn scene(scenes: &[Scene], step: usize) -> (Style, Scene) {
    let style = if step < scenes.len() {
        Style::Pill
    } else {
        Style::Notch
    };
    (style, scenes[step % scenes.len()])
}

/// Focus session 2 of 4 with 18:42 left (running or paused), a short break
/// ready to start, or one running with 3:20 left.
fn demo_pomodoro(which: Pomo, now: Instant) -> Pomodoro {
    let mut p = Pomodoro::new(PomoSettings::default());
    if which == Pomo::Idle {
        return p;
    }
    if which == Pomo::Ready {
        p.skip(now);
        return p;
    }
    if which == Pomo::Break {
        // Focus 1 → short break, 1:40 in.
        p.skip(now);
        p.start_pause(now.checked_sub(Duration::from_secs(100)).unwrap_or(now));
        return p;
    }
    // Focus 1 → short break → focus 2.
    p.skip(now);
    p.skip(now);
    let started = now.checked_sub(Duration::from_secs(378)).unwrap_or(now);
    p.start_pause(started);
    if which == Pomo::Paused {
        p.start_pause(now);
    }
    p
}

fn local(d: NaiveDate, h: u32, m: u32) -> EventTime {
    let t = d.and_time(NaiveTime::from_hms_opt(h, m, 0).expect("valid time"));
    EventTime::At(
        Local
            .from_local_datetime(&t)
            .earliest()
            .expect("local time"),
    )
}

/// Today: an exhibition running over several days and a birthday (all
/// day), a review three hours before the current hour and a meeting the hour
/// after it (so the day view shows the "now" line between them, as in the
/// design), a coffee at 07.00 and a render from 22.00 past midnight: they
/// never fit one window, so the day view always has a second page ("+N
/// lagi", then "Kembali ke atas"). The rest of the week has an event most
/// days, one of them with a very long title.
fn demo_calendar(today: NaiveDate, p: &Palette) -> CalendarSnapshot {
    let day = |n: i64| {
        if n >= 0 {
            today + Days::new(n as u64)
        } else {
            today - Days::new(n.unsigned_abs())
        }
    };
    let ev = |id: &str, title: &str, start, end, color| CalEvent {
        id: id.into(),
        calendar_id: "primary".into(),
        title: title.into(),
        start,
        end,
        description: String::new(),
        color,
        editable: true,
    };
    // Keeps the review on today and the meeting before midnight.
    let h = Local::now().hour().clamp(3, 20);
    let events = vec![
        ev(
            "expo",
            "Pameran arsitektur Jakarta",
            EventTime::AllDay(day(-1)),
            EventTime::AllDay(day(3)),
            Some(p.sage),
        ),
        ev(
            "bday",
            "Ulang tahun Rani",
            EventTime::AllDay(today),
            EventTime::AllDay(day(1)),
            Some(p.plum),
        ),
        ev(
            "standup",
            "Kopi pagi dengan tim",
            local(today, 7, 0),
            local(today, 7, 45),
            Some(p.plum),
        ),
        ev(
            "review",
            "Review desain klien",
            local(today, h - 3, 0),
            local(today, h - 2, 30),
            None,
        ),
        ev(
            "rapat",
            "Rapat koordinasi MEP",
            local(today, h + 1, 0),
            local(today, h + 2, 0),
            Some(p.ochre),
        ),
        ev(
            "render",
            "Render final malam",
            local(today, 22, 0),
            local(day(1), 2, 0),
            None,
        ),
        ev(
            "survey",
            "Survey lokasi Kemang",
            local(day(1), 8, 30),
            local(day(1), 10, 0),
            Some(p.sage),
        ),
        ev(
            "present",
            "Presentasi",
            local(day(2), 13, 0),
            local(day(2), 14, 0),
            None,
        ),
        ev(
            "gym",
            "Gym",
            local(day(4), 7, 0),
            local(day(4), 8, 0),
            Some(p.plum),
        ),
        ev(
            "rab",
            "Kirim RAB",
            local(day(4), 16, 0),
            local(day(4), 17, 0),
            Some(p.ochre),
        ),
        ev(
            "family",
            "Keluarga",
            local(day(6), 11, 0),
            local(day(6), 13, 0),
            Some(p.sage),
        ),
        ev(
            "long",
            "Rapat koordinasi kontraktor, struktur, MEP dan pemilik proyek gedung kantor",
            local(day(-5), 9, 0),
            local(day(-5), 10, 0),
            None,
        ),
        ev(
            "site",
            "Cek progres lapangan",
            local(day(-12), 14, 0),
            local(day(-12), 15, 0),
            None,
        ),
    ];
    CalendarSnapshot {
        status: CalStatus::Ok,
        events,
        from: day(-30),
        to: day(30),
        fetched_at: Some(Local::now()),
    }
}

fn stamp(t: NaiveDateTime) -> i64 {
    Local
        .from_local_datetime(&t)
        .earliest()
        .map_or(0, |t| t.timestamp())
}

/// The design's four cards (newest first), then two more for the second
/// chunk: one with a single line, one from last year.
fn demo_notes(today: NaiveDate) -> Vec<Note> {
    let now = Local::now().timestamp();
    let at = |d: NaiveDate, h, m| stamp(d.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap()));
    let note = |id: &str, text: &str, updated| Note {
        id: id.into(),
        text: text.into(),
        updated,
        sticky: None,
    };
    vec![
        note(
            "n1",
            "Ide fasad\nKisi-kisi kayu vertikal, jarak 8 cm, bayangan sore jatuh ke ruang tamu.",
            now - 10 * 60,
        ),
        note(
            "n2",
            "Daftar material\nBata ekspos, terakota lantai 30×30, kaca low-e, baja ringan.",
            now - 3600,
        ),
        note(
            "n3",
            "\n  Catatan rapat\nKlien minta skylight di tangga. Cek ulang beban atap, \
             lalu kirim revisi potongan dan detail kusen jendela kamar tidur.",
            at(today - Days::new(1), 16, 0),
        ),
        note(
            "n4",
            "Belanja\nKopi, kertas kalkir A2, pensil 2B, cutter.",
            at(today - Days::new(4), 8, 0),
        ),
        note(
            "n5",
            "Perpanjang lisensi software",
            at(today - Days::new(9), 11, 0),
        ),
        note(
            "n6",
            "Arsip proyek lama\nPindahkan ke drive eksternal.",
            at(today - Days::new(400), 11, 0),
        ),
    ]
}

fn many_notes(today: NaiveDate) -> Vec<Note> {
    let base = stamp(today.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap()));
    (0..30)
        .map(|i| Note {
            id: format!("m{i}"),
            text: format!("Catatan nomor {}", i + 1),
            updated: base - i64::from(i) * 7 * 3600,
            sticky: None,
        })
        .collect()
}

/// Today: one overdue ("Terlewat"), one later, one daily (repeat icon);
/// tomorrow one; and a weekly one later in the week (a date heading).
fn demo_reminders(today: NaiveDate) -> Vec<Reminder> {
    let now = Local::now().naive_local();
    let whole = |t: NaiveDateTime| t.with_second(0).unwrap_or(t);
    // The two later ones stay before midnight, so they stay under today.
    let left = i64::from(24 * 60 - 1 - (now.hour() * 60 + now.minute()));
    let later = |want: i64, share: i64, min: i64| {
        now + chrono::Duration::minutes(want.min(left * share / 3).max(min))
    };
    let r = |id: &str, text: &str, at: NaiveDateTime, repeat| Reminder {
        id: id.into(),
        text: text.into(),
        due_unix: stamp(at),
        repeat,
        done: false,
        days: Vec::new(),
    };
    let on = |d: NaiveDate, h, m| d.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap());
    vec![
        r(
            "r1",
            "Minum vitamin",
            whole(now - chrono::Duration::hours(2)),
            Repeat::Once,
        ),
        r(
            "r2",
            "Kirim revisi denah lantai 2 ke klien",
            whole(later(90, 1, 1)),
            Repeat::Once,
        ),
        r(
            "r3",
            "Backup file proyek",
            whole(later(150, 2, 2)),
            Repeat::Daily,
        ),
        r(
            "r4",
            "Survey lokasi Kemang",
            on(today + Days::new(1), 9, 30),
            Repeat::Once,
        ),
        r(
            "r5",
            "Telepon vendor kaca soal kaca low-e untuk fasad depan dan skylight tangga",
            on(today + Days::new(3), 13, 0),
            Repeat::Weekly,
        ),
    ]
}

/// Twelve reminders over the coming days.
fn many_reminders(today: NaiveDate) -> Vec<Reminder> {
    let repeats = [Repeat::Once, Repeat::Daily, Repeat::Weekly];
    (0..12u32)
        .map(|i| {
            let day = today + Days::new(u64::from(i / 3));
            let at = day.and_time(NaiveTime::from_hms_opt(8 + (i % 3) * 4, 30, 0).unwrap());
            Reminder {
                id: format!("m{i}"),
                text: format!("Pengingat nomor {}", i + 1),
                due_unix: stamp(at),
                repeat: repeats[i as usize % 3],
                done: false,
                days: Vec::new(),
            }
        })
        .collect()
}

/// A 240×192 cover like the design's: a terracotta sky darkening downwards,
/// a pale sun, a thin horizon. Not square, to exercise the crop.
fn test_art() -> Vec<u8> {
    const W: u32 = 240;
    const H: u32 = 192;
    let row = W * 3; // 720 bytes: already 4-byte aligned
    let pixels = row * H;
    let mut bmp = Vec::with_capacity((54 + pixels) as usize);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(54 + pixels).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(W as i32).to_le_bytes());
    bmp.extend_from_slice(&(H as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&[0u8; 24]);
    let stops: [(f32, [f32; 3]); 3] = [
        (0.0, [0xE2 as f32, 0x68 as f32, 0x3C as f32]),
        (0.55, [0xB4 as f32, 0x45 as f32, 0x2A as f32]),
        (1.0, [0x5A as f32, 0x2A as f32, 0x1C as f32]),
    ];
    let sky = |t: f32| -> [f32; 3] {
        let (a, b) = if t < stops[1].0 {
            (stops[0], stops[1])
        } else {
            (stops[1], stops[2])
        };
        let k = (t - a.0) / (b.0 - a.0);
        std::array::from_fn(|i| a.1[i] + (b.1[i] - a.1[i]) * k)
    };
    let sun = [0xF2 as f32, 0xC2 as f32, 0x7A as f32];
    let line = [0xD8 as f32, 0xB8 as f32, 0xA0 as f32];
    // BMP rows run bottom-up.
    for y in (0..H).rev() {
        for x in 0..W {
            let (fx, fy) = (x as f32, y as f32);
            let mut c = sky((fy / (H - 1) as f32 + fx / (W - 1) as f32 * 0.15).min(1.0));
            // The sun's edge blends over a pixel, so it stays round scaled up.
            let (dx, dy) = (fx + 0.5 - W as f32 * 0.68, fy + 0.5 - H as f32 * 0.3);
            let edge = H as f32 * 0.19 - (dx * dx + dy * dy).sqrt();
            let cover = (edge + 0.5).clamp(0.0, 1.0);
            c = std::array::from_fn(|i| c[i] + (sun[i] - c[i]) * cover);
            if y == H * 78 / 100 {
                c = std::array::from_fn(|i| (c[i] + line[i]) / 2.0);
            }
            bmp.extend_from_slice(&[c[2] as u8, c[1] as u8, c[0] as u8]);
        }
    }
    bmp
}

/// What the window procedure of `--demo-notice-close` saw, in client pixels,
/// for the demo loop to act on (it owns the renderer that maps a point to a
/// target).
#[derive(Clone, Copy)]
enum LiveInput {
    Move(i32, i32),
    Leave,
    Up(i32, i32),
}

thread_local! {
    static LIVE_INPUT: std::cell::RefCell<Vec<LiveInput>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn live_push(input: LiveInput) {
    LIVE_INPUT.with(|q| q.borrow_mut().push(input));
}

fn live_take() -> Vec<LiveInput> {
    LIVE_INPUT.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

/// The client point of a mouse message's `lparam` (signed 16 bits each).
fn live_point(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam.0 & 0xFFFF) as i16 as i32,
        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}

/// The window procedure of `--demo-notice-close`: the plain demo's plus the
/// mouse (move, leave, left button up), queued for the loop.
unsafe extern "system" fn live_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = unsafe { TrackMouseEvent(&mut track) };
            let (x, y) = live_point(lparam);
            live_push(LiveInput::Move(x, y));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            live_push(LiveInput::Leave);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = live_point(lparam);
            live_push(LiveInput::Up(x, y));
            LRESULT(0)
        }
        _ => unsafe { wnd_proc(hwnd, msg, wparam, lparam) },
    }
}

/// The made-up notices `--demo-notice-close` cycles through: a chat toast
/// (with its logo), "Claude finished", a clean that finished, and a sticky
/// reminder (no ✕).
const LIVE_NOTICES: [DemoNotice; 4] = [
    DemoNotice::AppLogo,
    DemoNotice::ClaudeDone,
    DemoNotice::CleanDone,
    DemoNotice::Reminder,
];

/// A pause before the next made-up notice, so the end of one shows.
const LIVE_REFILL: Duration = Duration::from_millis(800);

/// `--demo-notice-close`: a real `IslandState` fed made-up notices, with the
/// pointer and the ✕ on the real path (`Event::Click(hit)` from the
/// renderer's hit test). Effects are printed, never run.
struct LiveNoticeClose {
    state: IslandState,
    /// The next made-up notice to push, by place in `LIVE_NOTICES`.
    next: usize,
    /// When no notice showed first (the queue refills `LIVE_REFILL` later).
    empty_since: Option<Instant>,
    pointer_on: bool,
    hot: bool,
    fade: CloseFade,
    /// `DI_DEMO_CLOSE`: the fade frozen at this alpha while the ✕ is on.
    frozen: Option<f32>,
}

impl LiveNoticeClose {
    fn new(fullscreen: bool, frozen: Option<f32>, now: Instant) -> Self {
        let mut state = IslandState::new(Local::now().date_naive(), PomoSettings::default());
        if fullscreen {
            state.apply(Event::Fullscreen(true), now);
        }
        Self {
            state,
            next: 0,
            empty_since: None,
            pointer_on: false,
            hot: false,
            fade: CloseFade::settled(0.0, now),
            frozen,
        }
    }

    /// The pointer and clicks seen since the last frame, the notices'
    /// expiry, the refill and the ✕'s fade target.
    fn step(&mut self, renderer: &Renderer, now: Instant) {
        for input in live_take() {
            match input {
                LiveInput::Move(x, y) => {
                    let (x, y) = (x as f32, y as f32);
                    self.pointer_on = matches!(renderer.under(x, y), Under::Island(_));
                    self.hot = renderer.hit_test(x, y) == Some(Hit::NoticeClose);
                }
                LiveInput::Leave => {
                    self.pointer_on = false;
                    self.hot = false;
                }
                LiveInput::Up(x, y) => {
                    if let Some(hit) = renderer.hit_test(x as f32, y as f32) {
                        let effects = self.state.apply(Event::Click(hit), now);
                        eprintln!("effects: {effects:?}");
                    }
                }
            }
        }
        self.state.apply(Event::Tick, now);
        if matches!(self.state.view(), View::Notice(_)) {
            self.empty_since = None;
        } else {
            let since = *self.empty_since.get_or_insert(now);
            if now.duration_since(since) >= LIVE_REFILL {
                let mut notice =
                    demo_notice(LIVE_NOTICES[self.next % LIVE_NOTICES.len()], renderer);
                notice.key = format!("demo-live-{}", self.next);
                self.next += 1;
                self.empty_since = None;
                self.state.apply(Event::Notice(notice), now);
            }
        }
        let to = if self.pointer_on && self.state.notice_close_shown() {
            1.0
        } else {
            0.0
        };
        if self.fade.to != to {
            let ms = close_fade_ms(true, Motion::Full, false);
            self.fade.retarget(to, now, ms);
        }
    }

    fn view(&self) -> View {
        self.state.view()
    }

    /// Bata for the island's own notices, the app's logo for a toast.
    fn robot(&self) -> Option<Expr> {
        match self.state.view() {
            View::Notice(n) if matches!(n.kind, NoticeKind::App { toast: Some(_), .. }) => None,
            View::Notice(_) => Some(Expr::Happy),
            _ => None,
        }
    }

    fn close_view(&self, now: Instant) -> Option<NoticeCloseView> {
        let alpha = match self.frozen {
            Some(a) if self.fade.to > 0.0 => a.clamp(0.0, 1.0),
            Some(_) => 0.0,
            None => self.fade.alpha(now),
        };
        (alpha > 0.0).then_some(NoticeCloseView {
            alpha,
            hot: self.hot,
        })
    }
}

/// `--demo-notice-close` (debug builds): see the module docs.
pub fn run_notice_close() {
    let scene = Scene {
        kind: ShapeKind::Notice,
        ..notice("notice-close-live", DemoNotice::AppLogo)
    };
    let fullscreen = std::env::var("DI_DEMO_FULLSCREEN").is_ok_and(|v| v.trim() == "1");
    let frozen: Option<f32> = env("DI_DEMO_CLOSE");
    let live = LiveNoticeClose::new(fullscreen, frozen, Instant::now());
    run_scenes_with(&[scene], Some(live));
}

/// Dispatches pending messages; false once WM_QUIT arrives.
fn pump_messages() -> bool {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// The Pintasan page's tiles: the default eleven (the nine built-ins, Vale's
/// Settings and the Shelf), or a made-up own set (a few built-ins, a page
/// tile, a web address, a folder, an app) for the custom scene.
fn scene_tiles(scene: &Scene) -> Vec<crate::tiles::Tile> {
    let mut tiles = ShortcutTiles::default();
    if scene.custom_tiles {
        for i in [7, 6, 5, 4] {
            tiles.toggle(i);
        }
        // Index 10 is the Calendar page tile, off by default.
        tiles.toggle(10);
        for (label, kind, target) in [
            ("Gmail", CustomKind::Url, "https://mail.google.com"),
            ("Proyek", CustomKind::Folder, r"D:\Proyek"),
            ("Editor", CustomKind::App, r"C:\Tools\editor.exe"),
            ("Docs", CustomKind::Url, "https://docs.google.com"),
        ] {
            if let Some(tile) = CustomTile::new(label, kind, target) {
                tiles.add(tile);
            }
        }
    }
    tiles.visible()
}

/// Made-up volumes for the Mixer scene: never the machine's own.
fn demo_mixer() -> Vec<crate::audio::MixerRow> {
    use crate::audio::{MixerRow, Target};
    let app = |name: &str, level: f32, muted: bool| MixerRow {
        target: Target::App(name.to_lowercase()),
        name: name.into(),
        level,
        muted,
    };
    vec![
        MixerRow {
            target: Target::Master,
            name: String::new(),
            level: 0.62,
            muted: false,
        },
        app("Spotify", 0.8, false),
        app("Chrome", 0.45, false),
        app("Teams", 0.2, true),
    ]
}

/// What the quick search's demo scene has typed.
const DEMO_SEARCH_QUERY: &str = "kal";

/// What the `search-files` scene has typed.
const DEMO_FILES_QUERY: &str = "gam";

/// The demo's results, from a made-up list of apps (never the real Start
/// Menu) and, with `files`, made-up files and folders (never the real Known
/// Folders): what `search::search` makes of `query`.
fn demo_search_results(query: &str, files: bool) -> Vec<crate::search::SearchResult> {
    use crate::search::{AppEntry, Labels};
    let names: &[&str] = if files {
        &[
            "Game Bar",
            "Gaming Hub",
            "Kalender",
            "Notepad",
            "Visual Studio Code",
        ]
    } else {
        &[
            "Kalender",
            "Kalkulator",
            "Kamera",
            "Notepad",
            "Visual Studio Code",
        ]
    };
    let apps: Vec<AppEntry> = names
        .iter()
        .map(|name| AppEntry {
            name: (*name).into(),
            path: format!(r"C:\Demo\{name}.lnk"),
        })
        .collect();
    let found = if files {
        demo_search_files()
    } else {
        Vec::new()
    };
    let lang = crate::i18n::lang();
    let web_title = move |q: &str| crate::i18n::search_web_title(lang, q);
    let size = move |bytes: u64| crate::i18n::search_file_size(lang, bytes);
    crate::search::search(
        query,
        &apps,
        &found,
        lang,
        &Labels {
            app: tr(Key::SearchKindApp),
            setting: tr(Key::SearchKindSetting),
            copy: tr(Key::SearchCopy),
            web: tr(Key::SearchKindWeb),
            web_title: &web_title,
            root: &crate::i18n::search_root,
            size: &size,
        },
    )
}

/// Made-up files and folders for the `search-files` scene, as the walk would
/// have found them.
fn demo_search_files() -> Vec<crate::search_files::FileEntry> {
    use crate::search_files::{FileEntry, RootKind};
    let at = |secs: u64| Some(std::time::UNIX_EPOCH + Duration::from_secs(1_760_000_000 + secs));
    let entry = |name: &str, root, parent: Option<&str>, is_dir, size, modified| FileEntry {
        name: name.into(),
        lower: name.to_lowercase(),
        path: std::path::PathBuf::from(match parent {
            Some(p) => format!(r"C:\Demo\{p}\{name}"),
            None => format!(r"C:\Demo\{name}"),
        }),
        root,
        parent: parent.map(str::to_owned),
        is_dir,
        size,
        modified,
    };
    vec![
        entry(
            "Gambar kerja",
            RootKind::Documents,
            Some("Ruko Melati"),
            true,
            0,
            at(900),
        ),
        entry(
            "gambar-fasad-utara.png",
            RootKind::Pictures,
            Some("Ruko Melati"),
            false,
            2_516_582,
            at(800),
        ),
        entry(
            "gambar-detail-tangga.dwg",
            RootKind::Documents,
            Some(r"Ruko Melati\Gambar kerja"),
            false,
            186_000_000,
            at(700),
        ),
        entry(
            "gambar-lama.dwg",
            RootKind::Downloads,
            None,
            false,
            845_000,
            at(100),
        ),
    ]
}

/// What the details worker would have found for the demo's rows: drawn
/// icons (a flat tile per kind), a folder's count and an app's size.
fn demo_search_details()
-> std::collections::HashMap<std::path::PathBuf, crate::search_details::Detail> {
    use crate::search_details::Detail;
    let tile = |rgb: [u8; 3]| -> Thumb {
        let (w, h) = (32u32, 32u32);
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                // A rounded square with a lighter top edge, clear outside.
                let (dx, dy) = (
                    x.abs_diff(16).max(1) as f32 - 0.5,
                    y.abs_diff(16).max(1) as f32 - 0.5,
                );
                let inside = dx < 14.0 && dy < 14.0 && (dx * dx + dy * dy) < 17.0 * 17.0;
                let lift = if y < 12 { 24 } else { 0 };
                let px = |c: u8| c.saturating_add(lift);
                if inside {
                    bgra.extend([px(rgb[2]), px(rgb[1]), px(rgb[0]), 255]);
                } else {
                    bgra.extend([0, 0, 0, 0]);
                }
            }
        }
        Thumb { w, h, bgra }
    };
    let icon = |rgb| Some(tile(rgb));
    let mut details = std::collections::HashMap::new();
    let mut put = |path: &str, detail| {
        details.insert(std::path::PathBuf::from(path), detail);
    };
    put(
        r"C:\Demo\Game Bar.lnk",
        Detail {
            icon: icon([70, 160, 90]),
            install: Some(142 * 1024 * 1024),
            ..Detail::default()
        },
    );
    put(
        r"C:\Demo\Gaming Hub.lnk",
        Detail {
            icon: icon([120, 90, 200]),
            ..Detail::default()
        },
    );
    put(
        r"C:\Demo\Ruko Melati\Gambar kerja",
        Detail {
            icon: icon([230, 190, 90]),
            count: Some(18),
            ..Detail::default()
        },
    );
    put(
        r"C:\Demo\Ruko Melati\gambar-fasad-utara.png",
        Detail {
            icon: icon([90, 150, 220]),
            ..Detail::default()
        },
    );
    details
}

/// Made-up files for the shelf scene (and an empty shelf elsewhere): never
/// the machine's own. One of them is gone, as a card shows it, and two came
/// from a phone (they carry the "dari HP" tag).
fn demo_shelf(filled: bool) -> Vec<crate::shelf::ShelfCard> {
    if !filled {
        return Vec::new();
    }
    let mut cards: Vec<crate::shelf::ShelfCard> = demo_shelf_files()
        .into_iter()
        .filter(|c| c.name != "render.mp4")
        .take(5)
        .collect();
    for name in ["IMG_0531.png", "IMG_0533.png"] {
        cards.push(crate::shelf::ShelfCard {
            key: name.to_lowercase(),
            name: name.into(),
            kind: crate::shelf::ShelfKind::Image,
            stale: false,
            from_phone: true,
        });
    }
    cards
}

/// The phone link's made-up data for a scene: a QR code of an invented link,
/// files in different states. Owned here so that the
/// `PhoneView` can borrow it for the frame.
struct DemoPhone {
    mode: PhoneDemo,
    qr: crate::phone::QrMatrix,
    rows: Vec<crate::ui::phone_view::RowView>,
    /// Step 2's line once a phone is in ("Tersambung: Android").
    joined: String,
}

impl DemoPhone {
    fn new(mode: PhoneDemo) -> Self {
        use crate::shelf::ShelfKind;
        use crate::ui::phone_view::{RowStatus, RowView};
        let row = |name: &str, bytes: u64, kind, status| RowView {
            name: name.into(),
            size: bytes,
            kind,
            status,
        };
        let mib = |x: f64| (x * 1_048_576.0) as u64;
        DemoPhone {
            mode,
            joined: crate::i18n::phone_connected(
                crate::i18n::lang(),
                &[crate::phone::DeviceKind::Android],
            ),
            qr: crate::phone::qr::make("http://192.168.18.42:47390/s/AbCdEfGhIjKlMnOpQrStUv")
                .expect("a link fits a QR code"),
            rows: vec![
                row("IMG_0531.png", mib(5.1), ShelfKind::Image, RowStatus::Done),
                row("IMG_0533.png", mib(1.7), ShelfKind::Image, RowStatus::Done),
                row(
                    "VID_2026-11-04.mp4",
                    mib(48.2),
                    ShelfKind::Video,
                    RowStatus::Receiving(62),
                ),
                row("scan-denah.pdf", mib(12.4), ShelfKind::Document, RowStatus::Failed),
            ],
        }
    }

    fn view(&self) -> Option<crate::ui::phone_view::PhoneView<'_>> {
        use crate::model::PhoneStage;
        use crate::ui::phone_view::{CodeView, PhoneView, QrView, RecvView, Warn};
        let stage = match self.mode {
            PhoneDemo::No => return None,
            PhoneDemo::Qr
            | PhoneDemo::QrJoined
            | PhoneDemo::QrExpired
            | PhoneDemo::Public
            | PhoneDemo::NoLan => PhoneStage::Qr,
            PhoneDemo::Receive => PhoneStage::Receiving,
        };
        let qr = match self.mode {
            PhoneDemo::NoLan => QrView::Warning(Warn::NoLan),
            PhoneDemo::Public => QrView::Code(CodeView {
                qr: &self.qr,
                network: "Kenzie_5G",
                addr: "192.168.18.42:47390",
                remaining: Some(292),
                public: true,
                joined: "",
            }),
            _ => QrView::Code(CodeView {
                qr: &self.qr,
                network: "Rumah-5G",
                addr: "192.168.18.42:47390",
                remaining: (self.mode != PhoneDemo::QrExpired).then_some(292),
                public: false,
                joined: if self.mode == PhoneDemo::QrJoined {
                    &self.joined
                } else {
                    ""
                },
            }),
        };
        Some(PhoneView {
            stage,
            qr,
            recv: RecvView {
                rows: &self.rows,
                chip: "Android",
                files: 2,
                bytes: self.rows[0].size + self.rows[1].size,
            },
        })
    }
}

/// Twelve made-up cards, the shelf's most: the first six are the `shelf`
/// scene's.
fn demo_shelf_files() -> Vec<crate::shelf::ShelfCard> {
    use crate::shelf::{ShelfCard, ShelfKind};
    let card = |name: &str, kind, stale| ShelfCard {
        key: name.to_lowercase(),
        name: name.into(),
        kind,
        stale,
        from_phone: false,
    };
    vec![
        card(
            "denah-lantai-2-revisi-final.pdf",
            ShelfKind::Document,
            false,
        ),
        card("foto-lokasi.png", ShelfKind::Image, false),
        card("Gambar kerja", ShelfKind::Folder, false),
        card("rab-v3.xlsx", ShelfKind::Document, false),
        card("render.mp4", ShelfKind::Video, false),
        card("arsip-lama.zip", ShelfKind::Archive, true),
        card("sketsa-fasad.dwg", ShelfKind::Document, false),
        card("moodboard.jpg", ShelfKind::Image, false),
        card("Referensi", ShelfKind::Folder, false),
        card("notulen-rapat.docx", ShelfKind::Document, false),
        card("walkthrough.mov", ShelfKind::Video, false),
        card("backup-proyek.7z", ShelfKind::Archive, false),
    ]
}

/// How long a shelf motion scene plays, in ms (`DI_DEMO_SHELF` is a
/// fraction of it).
fn shelf_scene_ms(name: &str) -> Option<u64> {
    match name {
        "shelf-enter" => Some(SHELF_ENTER_MS + 2 * SHELF_STAGGER_MS),
        "shelf-exit" | "shelf-move" => Some(SHELF_MOVE_MS),
        _ => None,
    }
}

/// The shelf motion scenes: the cards after the change and the motion that
/// leads to them, `elapsed_ms` into it (rebuilt on every frame, so it
/// holds no clock of its own). `None` for any other scene.
fn demo_shelf_motion(
    name: &str,
    style: Style,
    strip: bool,
    now: Instant,
    elapsed_ms: u64,
) -> Option<(Vec<crate::shelf::ShelfCard>, ShelfMotion)> {
    let (before, after): (Vec<usize>, Vec<usize>) = match name {
        "shelf-enter" => ((0..3).collect(), (0..6).collect()),
        "shelf-exit" => ((0..6).collect(), vec![0, 2, 3, 4, 5]),
        "shelf-move" => ((0..9).collect(), vec![0, 1, 3, 4, 5, 6, 7, 8]),
        _ => return None,
    };
    let files = demo_shelf_files();
    let pick = |at: &[usize]| -> Vec<crate::shelf::ShelfCard> {
        at.iter().map(|&i| files[i].clone()).collect()
    };
    let shape = super::geometry::shape_for(style, ShapeKind::Panel);
    let rects = |n: usize| -> (Vec<super::geometry::Rect>, usize) {
        let l = super::panel_layout::shelf_layout(shape, strip, n);
        (l.cards.iter().map(|c| c.card).collect(), l.lines)
    };
    let (old, new) = (pick(&before), pick(&after));
    let start = now
        .checked_sub(Duration::from_millis(elapsed_ms))
        .unwrap_or(now);
    let ((old_rects, old_lines), (new_rects, _)) = (rects(old.len()), rects(new.len()));
    let mut motion = ShelfMotion::default();
    motion.on_change(&old, &old_rects, old_lines, &new, &new_rects, start, true);
    Some((new, motion))
}
