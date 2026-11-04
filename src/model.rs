use std::path::PathBuf;
use std::time::Instant;

use chrono::NaiveDate;

use crate::arrange::ArrangeEvent;
use crate::audio::MixerRow;
use crate::calendar::CalendarSnapshot;
use crate::clean::{CleanCat, CleanEvent, CleanMode};
use crate::cli::CliEvent;
use crate::clipboard::image::Thumb;
use crate::config::{OpenMode, ToastStyle};
use crate::devices::DevicesEvent;
use crate::greeting::DayPeriod;
use crate::lyrics::LyricsFound;
use crate::mole::{MoleEvent, MoleOpt, MoleOpts};
use crate::pages::PanelPages;
use crate::pomodoro::PomoSettings;
use crate::screenshot::{FileCandidate, FileOutcome};
use crate::search::AppEntry;
use crate::search_details::Detail;
use crate::search_files::FileEntry;
use crate::system::SystemSample;
use crate::tiles::{CustomTile, ShortcutTiles};
use crate::update::UpdateEvent;

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    /// Identifies the notice for de-duplication and tests; not shown to the user.
    pub key: String,
    pub source: String,
    pub title: String,
    pub body: String,
    pub kind: NoticeKind,
}

/// A Windows toast as the poller caught it, before anything decides where it
/// goes (`notif::route_toast`): the notch notice it makes, when it was
/// caught, and the app's logo the first time this run it was seen.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub notice: Notice,
    /// Unix seconds.
    pub at: i64,
    /// The logo key (`notif::logo_key`) and the picture, at most 48 x 48.
    pub logo: Option<(String, Thumb)>,
}

/// What a notice made from a real Windows toast carries
/// (`sources::notifications::to_notice` only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToastOrigin {
    /// The web site of a browser notification, when Windows gave it
    /// ("web.telegram.org").
    pub site: Option<String>,
}

/// `Notice::key` of a copy preview of an image: its badge spot is the
/// thumbnail, never Bata.
pub const CLIP_IMAGE_KEY: &str = "clip-preview-image";
/// `Notice::key` of a copy preview of text or files.
pub const CLIP_PREVIEW_KEY: &str = "clip-preview";

#[derive(Clone, Debug, PartialEq)]
pub enum NoticeKind {
    App {
        app_id: Option<String>,
        /// Set only by `sources::notifications::to_notice`: this notice comes
        /// from a real Windows toast, so the notch shows the app's logo and
        /// name. Island-owned App notices carry `None`.
        toast: Option<ToastOrigin>,
    },
    ClaudeDone {
        session_id: String,
    },
    /// Sticky: stays until the same session shows activity or finishes.
    ClaudeWaiting {
        session_id: String,
    },
    BatteryLow,
    /// A pomodoro phase ended; clicking it starts the next phase.
    Pomodoro,
    /// Sticky like `ClaudeWaiting`: a reminder whose time came. Clicking it
    /// marks it done (or moves a repeating one to its next time).
    Reminder {
        id: String,
    },
    /// The cat's greeting for a part of the day (`period` picks its face and
    /// words): events and reminders, from Bata. Shows `GREETING_DURATION`,
    /// never in fullscreen.
    Greeting {
        period: DayPeriod,
    },
    /// A fixed drive below the disk limit, or RAM above its limit for a
    /// minute (`system::WarnMonitor`). Clicking it opens the panel on the
    /// System page.
    SystemLow,
    /// The cleaner finished: what it freed, measured on the drive (Bata
    /// happy). Temporary; a click only dismisses it.
    CleanDone,
    /// A Mole clean finished: the free space the island measured (Bata
    /// happy, a "Log" chip). A click opens the cleaner on Mole's log.
    MoleDone,
    /// A Mole clean failed or stopped (Bata low, "Log"). Sticky like a
    /// reminder: it stays until clicked, which opens Mole's log.
    MoleFailed,
    /// "Disalin": an entry was copied from the island (or, with the setting,
    /// any copy). Short (`COPIED_DURATION`); a newer one replaces it.
    Copied,
    /// A copy made in another app: a preview of what is now in the history
    /// (entry `id`: text lines, an image thumbnail or the files), with a
    /// "Riwayat" chip. Lasts `CLIP_NOTICE_DURATION`; only the latest is kept
    /// (it replaces a `Copied` too). A click opens the Clipboard page.
    ClipPreview {
        id: u64,
    },
    /// The big screenshot preview (`ShapeKind::NoticeShot`): its thumbnail,
    /// size and Salin lagi / Simpan / Hapus. `shot` names `App`'s screenshot
    /// state; it shows `secs` from when it becomes current (never expiring
    /// under the pointer), and only a true full-time expiry with
    /// `delete_if_ignored` ("Jika diabaikan: Hapus") asks to delete.
    Screenshot {
        shot: u64,
        secs: u32,
        delete_if_ignored: bool,
    },
    /// "Disimpan · Pictures\Screenshots" after Simpan (Bata happy).
    ShotSaved,
    /// "Dihapus · clipboard dikosongkan" after Hapus (Bata idle).
    ShotDeleted,
    /// A flash drive or disk was plugged in: "Buka" opens Explorer on its
    /// first drive letter.
    DevicePlugged {
        letter: char,
    },
    /// Vale's Eject finished: "Aman dicabut". Temporary; a click dismisses.
    DeviceSafe,
    /// Susun arranged the windows: "Tersusun · n jendela", with "Batalkan"
    /// and (after a pick, not a saved run) "Simpan…". Lasts about 3 s, held
    /// under the pointer and while the name field is up.
    Arranged {
        save: ArrangeSave,
    },
    /// "Vale 3.11.0 tersedia" with the chips "Update" and "Nanti" (`version`
    /// is X.Y.Z). Sticky (one slot, any version): it stays until "Update"
    /// installs it, "Nanti" sets it aside, or the install starts elsewhere.
    UpdateAvailable {
        version: String,
    },
    /// "Mengunduh pembaruan… 42%": one at a time, replaced in place as the
    /// percent moves, gone 15 s after the last one.
    UpdateProgress {
        pct: u8,
    },
    /// "Update gagal: <alasan>" with the chip "Lihat rilis" (the releases
    /// page). A click opens it.
    UpdateFailed,
    /// "Vale sudah versi terbaru", "Diperbarui ke X.Y.Z", the debug-build
    /// note: temporary, a click dismisses.
    UpdateInfo,
}

/// What the Arranged notice offers beside "Batalkan".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrangeSave {
    /// A saved arrangement ran: undo only.
    None,
    /// "Simpan…".
    Offer,
    /// Twelve are saved already: "Simpan…" is off and says the list is full.
    Full,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub art: Option<Vec<u8>>,
    /// The shown session's AppUserModelId. Browser tabs share one, so it is
    /// a label for the log, not a way to find the session.
    pub source_app: String,
    /// An id the watcher gives the shown session, 0 when unknown (demo,
    /// tests). Transport commands go to the session with this key.
    pub session_key: u64,
    /// The shown session lets a seek through (`IsPlaybackPositionEnabled`);
    /// false when unknown or unreadable. The lyrics card shows its click-to-
    /// seek affordance only with it.
    pub can_seek: bool,
}

/// Where the current track's playback was at a known moment, from the media
/// session's timeline. While it plays, the position at `now` is `position_ms
/// + (now − updated) × rate` (`lyrics::position_at`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timeline {
    pub position_ms: u64,
    /// The track's length; 0 when the player does not report one.
    pub end_ms: u64,
    /// When `position_ms` was true.
    pub updated: Instant,
    /// The playback rate (1.0 normally; a player may play faster or slower).
    pub rate: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `None` means no media session is active.
    Media(Option<MediaInfo>),
    /// The session's timeline (seek, track change, play/pause); `None` when
    /// it reports none. Sent apart from `Media` so a seek never resends art.
    MediaTimeline(Option<Timeline>),
    /// Synced lyrics for the current track exist (`App` decides); the
    /// compact media view may show them.
    LyricsReady(bool),
    /// The "Lirik di notch" setting, at startup and when the settings window
    /// changes it.
    LyricsSetting(bool),
    /// A lookup by the lyrics worker finished; `App` keeps it, the reducer
    /// ignores it.
    Lyrics(LyricsFound),
    Notice(Notice),
    /// A fresh Windows toast from the poller (never muted or filtered
    /// there). `App` routes it: the notch (`Notice`) and the Notifikasi
    /// history; the reducer ignores it.
    Toast(Toast),
    /// A worker of the self-update (`update.rs`): a check, the download's
    /// progress or its end. `App` handles it; the reducer ignores it.
    Update(UpdateEvent),
    ClaudeActivity {
        session_id: String,
    },
    Fullscreen(bool),
    Hover(bool),
    Click(Hit),
    /// Raw `WM_MOUSEWHEEL` delta: +120 per notch away from the user.
    Wheel(i32),
    /// A sideways flick (a touchpad's two-finger swipe, a tilt wheel) over
    /// the open panel: exactly one page, right (`next`) or left, in the
    /// order and with the skips of the vertical wheel. `App` makes the flick
    /// (one per scroll) and decides where it counts; the reducer applies the
    /// wheel's own rules (the sticky reminder's peek, never the search).
    PageFlick {
        next: bool,
    },
    /// A note, event or reminder editor window opened (true) or closed (false).
    EditorOpen(bool),
    /// The local date rolled over.
    Today(NaiveDate),
    /// The reminder was edited, deleted or dismissed elsewhere: its sticky
    /// notice goes.
    ReminderCleared(String),
    /// A check found no newer release (the one offered was pulled): `App`
    /// forgot it, so the sticky update offer goes too.
    UpdateWithdrawn,
    /// The CLI page's input has the keyboard (true) or lost it: like an open
    /// editor, it keeps the panel open meanwhile.
    CliFocus(bool),
    /// The Clipboard page's search field has the keyboard (true) or lost it;
    /// it keeps the panel open like the CLI input.
    ClipFocus(bool),
    /// The Notifikasi page's hidden input has the keyboard (true) or lost
    /// it; it keeps the panel open like the CLI input.
    NotifFocus(bool),
    /// The optional global hotkey: opens the panel on the Clipboard page
    /// (without taking the foreground), or closes it when it shows that.
    ClipHotkey,
    /// The hotkey's panel was not visited in time: it closes.
    ClipHoldEnd,
    /// The quick search's hotkey: opens it over the page shown (and takes the
    /// keyboard for its field), or closes it when it is open.
    SearchHotkey,
    /// The tray's "Cari…": the quick search opens, or, when it is open
    /// already, its field takes the keyboard again (never a close).
    SearchOpen,
    /// The quick search closes: Esc, Enter on a result, the keyboard leaving.
    SearchClose,
    /// The Start Menu's apps, indexed on a background thread. `App` keeps
    /// them; the reducer has no use for them.
    SearchIndex(Vec<AppEntry>),
    /// The names under the user's Desktop, Documents, Downloads, Pictures,
    /// Videos and Music, walked on a background thread. `App` keeps them; the
    /// reducer has no use for them.
    SearchFiles(Vec<FileEntry>),
    /// What the details worker found for one result row (its icon, a
    /// folder's count, an app's size). `generation` is the request's; `App`
    /// keeps it, the reducer has no use for it.
    SearchDetail {
        generation: u64,
        key: PathBuf,
        detail: Detail,
    },
    /// Enter in the search field copied an entry: the panel closes like
    /// after a shortcut, and the keyboard goes back.
    ClipCommit,
    /// Scroll the CLI output `lines` up (negative: down), at most `max` lines
    /// above the newest: the wheel over the output, PageUp and PageDown.
    CliScroll {
        lines: i32,
        max: usize,
    },
    /// This many new output lines arrived; a view scrolled up stays on the
    /// lines it shows.
    CliOutput(usize),
    /// Scroll Mole's log `lines` up (negative: down), at most `max` lines
    /// above the newest: the wheel over the log while the cleaner shows it.
    MoleLogScroll {
        lines: i32,
        max: usize,
    },
    /// This many new Mole log lines arrived; a view scrolled up stays on the
    /// lines it shows.
    MoleLogLines(usize),
    /// A calendar sync result; `App` keeps it, the reducer ignores it.
    Calendar(CalendarSnapshot),
    /// Output or the end of a CLI command; `App` keeps it, the reducer ignores it.
    Cli(CliEvent),
    /// The open File Explorer folders, the last active first, from the
    /// reader `request` started; `App` keeps them, the reducer ignores them.
    ExplorerFolders {
        request: u64,
        folders: Vec<PathBuf>,
    },
    /// A reading from the system sampler (`system::Sampler`); `App` keeps
    /// it, the reducer ignores it.
    SystemSample(SystemSample),
    /// A scan or a clean from the cleaner thread (`clean::Cleaner`); `App`
    /// keeps it, the reducer ignores it.
    Clean(CleanEvent),
    /// The clean the panel asked for is over: its rows take clicks again.
    CleanFinished,
    /// The scan `Effect::OpenCleaner` asked for could not reach the cleaner
    /// thread: nothing is measured, so the next showing asks again.
    CleanScanFailed,
    /// From the Mole thread (`mole::MoleWorker`); `App` keeps it and tells
    /// the reducer what it needs below.
    Mole(MoleEvent),
    /// Mole was looked for after `Effect::MoleDetect`: found or not.
    MoleFound(bool),
    /// A dry-run with these options ended; `ok` when it finished well (a
    /// `mole::Preview` exists for them).
    MolePreviewed {
        opts: MoleOpts,
        ok: bool,
    },
    /// The confirmed Mole clean ended (`ok`: everything ran well).
    MoleFinished {
        ok: bool,
    },
    /// The cleaner's remembered mode (`clean_mode`), at startup.
    CleanModeSetting(CleanMode),
    /// "Mode buka" (startup and the settings window).
    OpenMode(OpenMode),
    /// "Notifikasi saat notch dipakai" (startup, the settings window and a
    /// hand edit): Full turns a tucked toast into the full notice at once.
    ToastStyle(ToastStyle),
    /// The pointer is on the busy toast's own spot (its strip, its logo slot,
    /// or the peeked full toast), or left it. Drives the peek and the strip's
    /// hold.
    ToastPointer(bool),
    /// A mouse button went down outside the island while the panel was kept
    /// open: it closes.
    Dismiss,
    /// "Halaman panel", at startup and when the settings window changes it.
    PanelPages(PanelPages),
    /// "Pintasan", at startup and when the settings window changes it.
    ShortcutTiles(ShortcutTiles),
    /// The Mixer's rows as the audio thread read them (`None`: no output
    /// device), with the number of the scan that asked. `App` keeps them,
    /// the reducer ignores it.
    Audio(u64, Option<Vec<MixerRow>>),
    /// The drive scanner's answer (`devices_win::Devices`); `App` keeps it,
    /// the reducer ignores it.
    Devices(DevicesEvent),
    /// A mover or icon worker of Susun (`arrange_win`); `App` keeps it, the
    /// reducer ignores it.
    Arrange(ArrangeEvent),
    /// The optional Devices hotkey: opens the panel on the Devices page
    /// (without taking the foreground), or closes it when it shows that.
    DevicesHotkey,
    /// The optional Susun hotkey: opens the panel on the Susun page at the
    /// template step (without taking the foreground), or closes it when it
    /// shows that.
    ArrangeHotkey,
    /// The pick step is running (true) or over (false): it keeps the panel
    /// open whatever the open mode is and a click outside does not close it.
    ArrangePick(bool),
    /// The Arranged notice's name field is up (true) or gone: the notice
    /// waits for it.
    ArrangeNaming(bool),
    /// The PrtSc key (when Settings lets Vale have it): Vale's own capture
    /// starts once an open panel has collapsed. A press with an overlay up
    /// is dropped by `App`, which knows the overlay.
    PrintScreen,
    /// The pointer asks row `index` of the Mixer for this level (0..=1).
    MixerLevel(usize, f32),
    /// Files are being dragged over the island (`true`: it opens on the
    /// shelf) or the drag left without a drop (`false`).
    ShelfDrag(bool),
    /// The same, as the OLE drop target reports it: `App` lights the drop
    /// zone, passes `true` on as `ShelfDrag(true)` and ends `false` a
    /// moment later (a drag that strays off the island and back keeps it).
    ShelfDragOver(bool),
    /// Files were dropped on the island. `App` puts them on the shelf, the
    /// reducer ignores it.
    ShelfDrop(Vec<std::path::PathBuf>),
    /// `App` put dropped files on the shelf. The panel opens on it and stays
    /// (a drop latches it like a click): the drag is over, so what held the
    /// panel for the drag ends, and the drop's own visit does not close it.
    ShelfDropped,
    /// The pointer left the island with the drop's visit still on and the
    /// dwell never fired (so no `Hover(false)` follows): the visit is over.
    /// `App` sends it when the reducer says `shelf_drop_here()`.
    ShelfDropLeft,
    /// The phone link told something (`phone::PhoneEvent`). `App` keeps all of
    /// it; the reducer only hears `PhoneAttention` and `PhoneStage`.
    /// What the phone link said, with the number of the session that said it
    /// (`App` ignores a session that is no longer the current one).
    Phone(u64, crate::phone::PhoneEvent),
    /// The phone link needs the owner now (files start to arrive): the panel opens on the shelf at that stage and
    /// stays, latched like a drop (`ShelfDropped`). Not in fullscreen.
    PhoneAttention(PhoneStage),
    /// `App` sets (or clears) the phone stage on the open shelf; it opens
    /// nothing.
    PhoneStage(Option<PhoneStage>),
    /// A folder check for screenshot `shot` ended: the Screenshots folder
    /// and the file that matched, if any. `App` keeps it, the reducer
    /// ignores it.
    ShotFolder {
        shot: u64,
        dir: Option<PathBuf>,
        found: Option<FileCandidate>,
    },
    /// "Simpan" for screenshot `shot` ended: the saved file's path, `None`
    /// when it failed. `written` is true when Simpan wrote that PNG itself,
    /// false when Windows' own file was found ("Sudah tersimpan"). `App`
    /// raises the notice; the reducer ignores it.
    ShotSaveDone {
        shot: u64,
        path: Option<PathBuf>,
        written: bool,
    },
    /// "Hapus" ran for `shot`: what became of the file, and whether the
    /// clipboard was emptied. `App` raises the notice; the reducer ignores it.
    ShotRecycled {
        shot: u64,
        file: FileOutcome,
        emptied: bool,
    },
    Tick,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Body,
    Prev,
    PlayPause,
    Next,
    CalPrev,
    CalNext,
    CalViewSel(CalView),
    CalDate(NaiveDate),
    /// Google event id.
    CalEvent(String),
    /// "+N lagi" on a day in the month or week view.
    CalMore(NaiveDate),
    CalAdd,
    CalConnect,
    PomoStartPause,
    PomoReset,
    PomoSkip,
    /// From the settings window; the panel page has no steppers.
    PomoAdjust(PomoField, i32),
    NoteNew,
    /// Note id.
    NoteOpen(String),
    /// The pin on a note's card: put it on the desktop, or take it off.
    NoteSticky(String),
    NotesMore,
    ReminderNew,
    /// Reminder id.
    ReminderOpen(String),
    RemindersMore,
    /// The Musik page's "Lirik" button: lyrics in the notch on or off.
    LyricsToggle,
    /// The Musik page's round button left of "Lirik": opens or closes the
    /// floating lyrics card. Only there while the lyrics setting is on.
    LyricsCard,
    /// The strip at the top of the open panel: the newest Claude waiting or
    /// due reminder notice.
    Strip,
    /// The due-reminder notice's "Selesai" chip (the closed notch only; the
    /// strip keeps `Hit::Strip`): marks it done.
    ReminderDone,
    /// The update offer's "Update" chip (notice or strip): install now.
    UpdateNow,
    /// The update offer's "Nanti" chip (notice or strip): clear the offer; the
    /// next check offers again.
    UpdateLater,
    /// The panel header's Settings button, at the top right of every page:
    /// opens Vale's Settings window.
    OpenSettings,
    /// The small logo beside a compact view that stands for a busy toast.
    ToastSlot,
    /// The ✕ at the right end of a timed full notice: closes it, opens nothing.
    NoticeClose,
    /// A result row of the quick search (its place in the list).
    SearchResult(usize),
    /// The back button left of the quick search's field: closes it, as Esc.
    SearchBack,
    /// A built-in tile on the Pintasan page.
    Shortcut(Shortcut),
    /// A page icon of the open panel's dock (the page dots, hovered).
    PanelPage(Page),
    /// A custom tile: its place in the tiles the page shows.
    CustomTile(usize),
    /// The Mixer page: a row's slider (the press itself is `App`'s, which
    /// knows where the pointer is) and its speaker icon, by the row's place.
    MixerSlider(usize),
    MixerMute(usize),
    /// The shelf: a card (opens the file), its X, and "Kosongkan", by the
    /// card's place on the shelf.
    ShelfCard(usize),
    ShelfRemove(usize),
    ShelfClear,
    /// The shelf's "HP" pill (opens the phone link), and the phone link's
    /// own controls while its view shows: its back button, "Buat QR baru",
    /// the small QR button of the receiving view, Batal and Selesai.
    ShelfPhone,
    PhoneBack,
    PhoneNewQr,
    PhoneShowQr,
    PhoneCancel,
    PhoneDone,
    /// The Devices page, by the card's place in the list as last drawn:
    /// "Eject" (and "Coba lagi", which asks the same again), the folder
    /// button, the info button, and "Eject semua".
    DeviceEject(usize),
    DeviceOpen(usize),
    DeviceDetail(usize),
    DeviceRetry(usize),
    DevicesEjectAll,
    /// Susun, template step: a monitor chip, a template card (built-ins
    /// first, then the user's) and a saved arrangement's play button, by
    /// place. Pick step: a window row by place in the list, and "Lewati
    /// ubin". The Arranged notice's "Batalkan" and "Simpan…".
    ArrangeMonitor(usize),
    ArrangeCard(usize),
    ArrangePlay(usize),
    ArrangeWindow(usize),
    ArrangeSkip,
    ArrangeUndo,
    ArrangeSave,
    /// The CLI page's terminal: the input takes the keyboard.
    CliInput,
    /// The CLI header's Stop button (the same as Ctrl+C).
    CliStop,
    /// The CLI header's enlarge / shrink button.
    CliExpand,
    /// The CLI header's folder chip (or the "dari Explorer" chip): opens or
    /// closes the folder menu.
    CliFolder,
    /// A folder row of the menu: the CLI moves there. `favorite`: the row is
    /// under Favorit (a right-click on it unstars it).
    CliFolderPick {
        path: PathBuf,
        favorite: bool,
    },
    /// A row's star: `on` stars the folder, off unstars it.
    CliFolderStar {
        path: PathBuf,
        on: bool,
    },
    /// Anywhere on the page outside the open menu: it closes.
    CliMenuDismiss,
    /// A drive's meter on the System page: Explorer opens on it.
    SystemDrive(char),
    /// A busiest-process row on the System page: Task Manager opens.
    SystemProcess,
    /// The System page's "Bersihkan" button: the cleaner opens over the
    /// page and scans.
    SystemClean,
    /// The cleaner's back button: the System page again.
    CleanBack,
    /// A cleaner row: its category ticked or not.
    CleanToggle(CleanCat),
    /// The cleaner's main button: the first click arms it ("Yakin?"), a
    /// second one within `CLEAN_CONFIRM` cleans.
    CleanGo,
    /// The cleaner's mode segment: "Pilih sendiri" or "Otomatis · Mole".
    CleanMode(CleanMode),
    /// "+ Sistem (admin)" or "+ Media game > 90 hari": a new preview runs.
    MoleOpt(MoleOpt),
    /// "Jalankan Mole": arms ("Yakin? Hapus permanen"); a second click
    /// within `CLEAN_CONFIRM`, after a preview with these options, cleans.
    /// After a run: "Pratinjau lagi".
    MoleGo,
    /// "Lindungi folder…": Mole's whitelist in a visible window.
    MoleProtect,
    /// "Cara memasang ↗": Mole's README in the browser.
    MoleInstall,
    /// "lihat log" (an unreadable group) or the log shown: the preview's
    /// log in place of the groups, and back.
    MoleShowLog,
    /// The Clipboard page's search field: it takes the keyboard.
    ClipSearch,
    /// The Notifikasi page: a row (entry id: opens its app), the selected
    /// row's "Buka", "Kecualikan app ini" and X, the header's "Jeda" and
    /// "Hapus semua", the list's background or the title (the page takes the
    /// keyboard) and the no-access "Buka pengaturan".
    NotifRow(u64),
    NotifOpen(u64),
    NotifMute(u64),
    NotifRemove(u64),
    NotifPause,
    NotifClear,
    NotifFocus,
    NotifSettings,
    /// A Clipboard card (entry id): copied again.
    ClipCard(u64),
    /// The selected card's buttons: pin or unpin, copy as plain text, delete.
    ClipPin(u64),
    ClipPlain(u64),
    ClipDelete(u64),
    /// The screenshot preview's buttons and its thumbnail (screenshot id):
    /// "Salin lagi", "Simpan", "Hapus", and opening the file.
    ShotCopy(u64),
    ShotKeep(u64),
    ShotDelete(u64),
    ShotOpen(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MediaCommand {
    Prev,
    /// Explicit, from the state the island shows: never a blind toggle.
    Play,
    Pause,
    Next,
    /// Jump to this position (milliseconds from the track's start): the
    /// lyrics card's click on a line.
    Seek(u64),
}

/// What the island knows about a caught toast when the user opens it: enough
/// to go to where it came from (`sources::notif_open`).
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTarget {
    /// The app's AppUserModelID, when Windows told it.
    pub app_id: Option<String>,
    /// The app's display name.
    pub source: String,
    /// The web site of a browser notification, when Windows gave it.
    pub site: Option<String>,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Bring forward where a caught toast came from.
    OpenTarget(OpenTarget),
    /// A Claude notice was clicked: bring forward the Claude Code session it
    /// came from (`sources::claude_open`). Nothing happens when the hook sent
    /// no origin; it never starts anything.
    OpenClaudeSession { session_id: String },
    Media(MediaCommand),
    PlaySound,
    SavePomodoro(PomoSettings),
    ConnectGoogle,
    /// Fetch events for `[from, to)`.
    RefreshCalendar {
        from: NaiveDate,
        to: NaiveDate,
    },
    /// `None` opens an empty editor for a new note.
    OpenNoteEditor(Option<String>),
    /// Note id: stick it to the desktop, or unstick it (a desktop sticky note).
    ToggleNoteSticky(String),
    /// `event_id: None` creates a new event on `date`.
    OpenEventEditor {
        event_id: Option<String>,
        date: NaiveDate,
    },
    /// `None` opens an empty editor for a new reminder.
    OpenReminderEditor(Option<String>),
    /// A due reminder's notice was clicked: done, or on to its next time.
    ReminderDone(String),
    /// The Musik page's lyric button changed the setting; save it.
    SaveLyrics(bool),
    /// The Musik page's lyrics card button: open or close the card (`App`
    /// applies `lyrics_card::card_next`).
    ToggleLyricsCard,
    /// Run a shortcut once the panel has collapsed.
    RunShortcut(Shortcut),
    /// Vale's own capture, whatever the "Alat screenshot" choice is (the
    /// PrtSc key), once the panel has collapsed.
    RunCapture,
    /// The same for a custom tile.
    RunCustom(CustomTile),
    /// Set the Mixer row's level / toggle its mute; `App` knows the rows.
    MixerLevel(usize, f32),
    MixerMute(usize),
    /// Open shelf card `i`'s file / take it off / empty the shelf; `App`
    /// keeps the shelf.
    ShelfOpen(usize),
    ShelfRemove(usize),
    ShelfClear,
    /// The phone link, as the hits of the same names; `App` keeps the server.
    /// `PhoneStart` is the "HP" pill (a session starts if none runs and the
    /// view shows), `PhoneBack` stops it only if no phone ever connected.
    PhoneStart,
    PhoneNewQr,
    PhoneCancel,
    PhoneStop,
    PhoneBack,
    /// Devices page: eject card `i`'s device / open Explorer on its first
    /// drive (the panel has collapsed) / expand or fold its details / eject
    /// every listed device. `App` keeps the list.
    DeviceEject(usize),
    DeviceOpen(usize),
    DeviceDetail(usize),
    DevicesEjectAll,
    /// Susun, as the hits of the same names.
    ArrangeMonitor(usize),
    ArrangeCard(usize),
    ArrangePlay(usize),
    ArrangeWindow(usize),
    ArrangeSkip,
    ArrangeUndo,
    ArrangeSave,
    /// Give the CLI input the keyboard (the island activates for it).
    FocusCliInput,
    /// The CLI input lets go of the keyboard; the window that had it
    /// before gets it back.
    ReleaseCliFocus,
    /// End the running command (Stop, Ctrl+C).
    StopCli,
    /// A folder picked in the menu: the CLI moves there (not while a
    /// command runs).
    CliChangeFolder(PathBuf),
    /// Star (`on`) or unstar a folder in the menu.
    CliFavorite {
        path: PathBuf,
        on: bool,
    },
    /// Explorer on this drive's root, once the panel has collapsed.
    OpenDrive(char),
    /// The cleaner opened: measure the categories.
    OpenCleaner,
    /// Clean these categories. Only after a confirmed second click.
    CleanDelete(Vec<CleanCat>),
    /// Remember the cleaner's mode (`clean_mode`).
    SaveCleanMode(CleanMode),
    /// Look for Mole (`Event::MoleFound` answers).
    MoleDetect,
    /// Mole's dry-run with these options; one running stops first.
    MolePreview(MoleOpts),
    /// Stop a running dry-run (the cleaner closed or its mode changed).
    MoleStopPreview,
    /// The confirmed Mole clean, with the options of the finished preview.
    /// Only on the armed button's second click.
    MoleClean(MoleOpts),
    /// `clean --whitelist` in a visible window.
    MoleWhitelist,
    /// Mole's README in the browser.
    OpenMoleInstall,
    /// Give the Clipboard search field the keyboard (the island activates).
    FocusClipSearch,
    /// The search field lets go; the window that had the keyboard gets it
    /// back.
    ReleaseClipFocus,
    /// Give the Notifikasi page's hidden input the keyboard (the island
    /// activates).
    FocusNotifInput,
    /// The page lets go; the window that had the keyboard gets it back.
    ReleaseNotifFocus,
    /// Something to do with a notification entry or the page's header.
    Notif(NotifAction),
    /// Give the quick search's field the keyboard (the island activates).
    FocusSearch,
    /// The quick search is gone: the keyboard goes back.
    ReleaseSearchFocus,
    /// Run result `n` of the quick search's list, then close it.
    SearchRun(usize),
    /// Something to do with a history entry.
    Clip(ClipAction),
    /// Something to do with the screenshot on preview.
    Shot(ShotAction),
    /// Open Vale's own Settings window (the panel header's Settings button).
    OpenSettings,
    /// The "Update" chip of the update notice: download, verify and swap in
    /// the release `App` knows.
    InstallUpdate,
    /// The failed update's chip: the releases page in the browser.
    OpenReleases,
}

/// What the screenshot preview asks for (screenshot id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotAction {
    /// "Salin lagi": the image onto the clipboard again; the file stays.
    Copy(u64),
    /// "Simpan": the file stays; "Disimpan" shows.
    Keep(u64),
    /// "Hapus" clicked: the file to the Recycle Bin, the clipboard emptied
    /// if it still holds the image, its history card gone.
    Delete(u64),
    /// The preview ran its full time untouched and was raised with "Jika
    /// diabaikan: Hapus". `App` deletes as for `Delete` only if that is
    /// still the setting now; otherwise the file is kept (Simpan).
    DeleteIgnored(u64),
    /// The thumbnail: the file in its default app.
    Open(u64),
}

/// What a Notifikasi row, its buttons or the header ask for (entry id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifAction {
    /// Mark read and, when the entry knows its app, bring that app forward.
    Open(u64),
    /// "Kecualikan app ini": the entry's app joins `muted_apps`.
    Mute(u64),
    /// The X: out of Vale's list only (never the Windows Action Center).
    Remove(u64),
    /// "Jeda".
    TogglePause,
    /// The confirmed "Hapus semua".
    ClearAll,
    /// The no-access button: Windows' notification privacy page.
    OpenSettings,
}

/// What a Clipboard card's click or button asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipAction {
    /// Onto the clipboard in its own form.
    Copy(u64),
    /// Onto the clipboard as text only (not for an image).
    CopyPlain(u64),
    /// Pin or unpin.
    TogglePin(u64),
    Delete(u64),
}

/// A Pomodoro setting the settings window's steppers can change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PomoField {
    Focus,
    ShortBreak,
    LongBreak,
    Sessions,
}

/// What the phone link shows over the Shelf page: the QR code (or why there
/// is none) and the files arriving. Like the cleaner over System it is not a
/// page: it comes with its own back button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhoneStage {
    Qr,
    Receiving,
}

/// Panel pages, in scroll order: Kalender, Pomodoro, Pengingat, Catatan,
/// Musik, Mixer, Clipboard, Rak, Sistem, Perangkat, then CLI and Pintasan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Calendar,
    Pomodoro,
    Reminders,
    Notes,
    Media,
    /// Mixer: the volume of the system and of each app playing sound.
    Mixer,
    /// The clipboard history.
    Clipboard,
    /// Notifikasi: the Windows toasts the island caught.
    Notifications,
    /// The file shelf: files left on the island to take out later.
    Shelf,
    /// Sistem: CPU, RAM, disks and the busiest processes.
    System,
    /// Perangkat: flash drives and external disks, with Eject.
    Devices,
    Cli,
    /// Susun: arranges the open windows into a tile layout.
    Arrange,
    /// Pintasan, the right-most page.
    Shortcuts,
    /// The quick search. Not one of `ALL`: only its hotkey opens it, the
    /// wheel never reaches it, and the page it came over comes back when it
    /// closes.
    Search,
}

impl Page {
    pub const ALL: [Page; 14] = [
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
    ];
}

/// The Pintasan page's tiles, in reading order (two rows of four).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Screenshot,
    Clipboard,
    Search,
    Emoji,
    Explorer,
    Calculator,
    TaskManager,
    WindowsSettings,
    Lock,
    /// Jumps straight to a panel page (never Search or Shortcuts).
    Go(Page),
}

impl Shortcut {
    pub const ALL: [Shortcut; 21] = [
        Shortcut::Screenshot,
        Shortcut::Clipboard,
        Shortcut::Search,
        Shortcut::Emoji,
        Shortcut::Explorer,
        Shortcut::Calculator,
        Shortcut::TaskManager,
        Shortcut::WindowsSettings,
        Shortcut::Lock,
        Shortcut::Go(Page::Calendar),
        Shortcut::Go(Page::Pomodoro),
        Shortcut::Go(Page::Reminders),
        Shortcut::Go(Page::Notes),
        Shortcut::Go(Page::Media),
        Shortcut::Go(Page::Mixer),
        Shortcut::Go(Page::Notifications),
        Shortcut::Go(Page::Shelf),
        Shortcut::Go(Page::System),
        Shortcut::Go(Page::Devices),
        Shortcut::Go(Page::Cli),
        Shortcut::Go(Page::Arrange),
    ];

    /// Whether a fresh install shows this tile: the original nine, the
    /// Shelf and Devices (11 tiles). The other page tiles (Susun
    /// included) are there to add.
    pub fn default_on(self) -> bool {
        !matches!(self, Shortcut::Go(p) if !matches!(p, Page::Shelf | Page::Devices))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalView {
    Day,
    Week,
    Month,
}

#[derive(Clone, Debug, PartialEq)]
pub enum View {
    /// Only in fullscreen.
    Hidden,
    /// The empty black shape.
    Idle,
    MediaCompact,
    /// The compact media view showing the current synced lyric line.
    MediaLyric,
    PomodoroCompact,
    Panel(Page),
    Notice(Notice),
}
