//! The island's two UI languages. Every user-facing string comes from here:
//! fixed text through `Key` (`tr` for the current language, `text` for a
//! given one), text with numbers or names through the small helpers below.
//!
//! The current language is one global, set by the app from `config.language`
//! at startup and when it changes. Strings are looked up when a frame is drawn
//! or a notice is made, so a change shows on the next redraw; notices already
//! made keep their text. Hook and CLI output stays Indonesian.

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::clean::CleanCat;
use crate::greeting::{DayPeriod, GreetCounts};
use crate::recorder::prefs::VideoQuality;
use crate::reminders::{self, Repeat};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    /// Bahasa Indonesia, the default.
    #[default]
    Id,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Id => "id",
            Lang::En => "en",
        }
    }

    /// "id" / "en", any case, with surrounding blanks.
    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_ascii_lowercase().as_str() {
            "id" => Some(Lang::Id),
            "en" => Some(Lang::En),
            _ => None,
        }
    }
}

impl Serialize for Lang {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

/// An unknown code (a typo in config.json) is the default language rather
/// than an invalid config, which would be backed up and replaced.
impl<'de> Deserialize<'de> for Lang {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let code = String::deserialize(d)?;
        Ok(Lang::from_code(&code).unwrap_or_default())
    }
}

#[cfg(not(test))]
mod current {
    use std::sync::atomic::{AtomicU8, Ordering};

    static LANG: AtomicU8 = AtomicU8::new(0);

    pub fn get() -> u8 {
        LANG.load(Ordering::Relaxed)
    }

    pub fn set(v: u8) {
        LANG.store(v, Ordering::Relaxed);
    }
}

/// Tests run on parallel threads: each gets its own language, so a test that
/// switches to English never changes the strings another test expects.
#[cfg(test)]
mod current {
    use std::cell::Cell;

    thread_local! {
        static LANG: Cell<u8> = const { Cell::new(0) };
    }

    pub fn get() -> u8 {
        LANG.with(Cell::get)
    }

    pub fn set(v: u8) {
        LANG.with(|l| l.set(v));
    }
}

pub fn set_lang(lang: Lang) {
    current::set(match lang {
        Lang::Id => 0,
        Lang::En => 1,
    });
}

pub fn lang() -> Lang {
    match current::get() {
        1 => Lang::En,
        _ => Lang::Id,
    }
}

/// Declares `Key`, `Key::ALL` and the table of both translations, so a key
/// without one of them does not compile.
macro_rules! keys {
    ($($name:ident => $id:literal, $en:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Key {
            $($name,)*
        }

        impl Key {
            #[cfg_attr(not(test), allow(dead_code))]
            pub const ALL: &[Key] = &[$(Key::$name,)*];
        }

        fn pair(key: Key) -> (&'static str, &'static str) {
            match key {
                $(Key::$name => ($id, $en),)*
            }
        }
    };
}

keys! {
    // Notice sources (their first letter is the badge).
    SourceCalendar => "Kalender", "Calendar";
    SourceBattery => "Baterai", "Battery";
    SourceNotification => "Notifikasi", "Notification";
    SourceReminder => "Pengingat", "Reminder";

    // Island notices.
    ClaudeDone => "Claude selesai", "Claude finished";
    ClaudeWaiting => "Claude butuh respon", "Claude needs you";
    BatteryLow => "Baterai lemah", "Low battery";
    FocusDone => "Fokus selesai", "Focus done";
    BreakDone => "Istirahat selesai", "Break done";
    NotesSaveFailed => "Gagal simpan catatan", "Couldn't save notes";
    NotesUnreadable => "notes.json tidak terbaca", "Can't read notes.json";
    NotesUnreadableBody => "Disimpan sebagai notes.json.bak saat menyimpan berikutnya",
        "Will be saved as notes.json.bak on the next save";
    Missed => "Terlewat", "Missed";
    MarkDone => "Selesai", "Done";
    RemindersSaveFailed => "Gagal simpan pengingat", "Couldn't save reminders";
    RemindersUnreadable => "reminders.json tidak terbaca", "Can't read reminders.json";
    RemindersUnreadableBody => "Disimpan sebagai reminders.json.bak saat menyimpan berikutnya",
        "Will be saved as reminders.json.bak on the next save";
    ConfigInvalid => "config.json tidak valid", "config.json is invalid";
    ConfigNotCopied => "Tidak bisa disalin — cek lagi isinya",
        "Couldn't copy it — check its contents";
    ConfigCheck => "Cek lagi isinya — lihat README", "Check its contents — see the README";
    ClientIdMissing => "Isi Google client ID dulu", "Add a Google client ID first";
    SeeReadme => "Lihat README", "See the README";
    ClientIdMissingBody => "Pengaturan → Integrasi, panduan ada di README",
        "Settings → Integrations, the guide is in the README";
    DndTitle => "Matiin popup Windows", "Turn off Windows pop-ups";
    DndBody => "Nyalain Jangan Ganggu: Pengaturan → Sistem → Pemberitahuan",
        "Turn on Do Not Disturb: Settings → System → Notifications";
    UserProfileUnset => "USERPROFILE tidak diset", "USERPROFILE is not set";

    // Calendar and Google.
    SaveEventFailed => "Gagal simpan acara", "Couldn't save event";
    GoogleScopes => "Izin Google belum lengkap", "Google permissions incomplete";
    GoogleScopesBody => "Centang kedua izin saat login", "Check both permissions when you sign in";
    GoogleRefused => "Google menolak", "Google refused";
    // Settings → Integrasi with Vale's own Google client.
    SignInGoogle => "Masuk dengan Google", "Sign in with Google";
    GoogleAdvanced => "Lanjutan: pakai Client ID sendiri", "Advanced: use your own Client ID";
    GoogleClientChanged => "Masuk Google lagi", "Sign in to Google again";
    GoogleClientChangedBody => "Cara masuk Google berganti, login lama tidak berlaku",
        "The Google sign-in changed, so the old login no longer works";
    ClientRejected => "Client ID atau secret ditolak — cek Pengaturan → Integrasi",
        "Client ID or secret rejected — check Settings → Integrations";
    BuiltInRejected => "Login Google Vale sedang ditolak — coba lagi nanti, atau pakai Client ID sendiri (Pengaturan → Integrasi → Lanjutan)",
        "Vale's Google sign-in is being refused — try again later, or use your own Client ID (Settings → Integrations → Advanced)";
    ApiDisabled => "Google Calendar API belum diaktifkan — lihat README",
        "Google Calendar API isn't enabled — see the README";
    NoTitle => "(Tanpa judul)", "(No title)";
    SaveFailed => "Gagal simpan", "Couldn't save";
    ReasonNotConfigured => "Google belum diatur", "Google isn't set up";
    ReasonNeedsLogin => "hubungkan Google dulu", "connect Google first";
    ReasonReconnect => "hubungkan ulang Google", "reconnect Google";
    ReasonScopes => "izin Google belum lengkap", "Google permissions incomplete";
    ReasonOffline => "tidak ada koneksi", "no connection";
    ReasonSyncStopped => "sinkron kalender berhenti", "calendar sync stopped";
    OauthConnected => "Berhasil terhubung. Kembali ke island ya.",
        "Connected. You can go back to the island.";
    OauthCancelled => "Login dibatalkan — tekan Masuk dengan Google di island untuk mencoba lagi.",
        "Sign-in cancelled — press Sign in with Google in the island to try again.";
    OauthRejected => "Google menolak Client ID — cek Pengaturan → Integrasi di island.",
        "Google rejected the Client ID — check Settings → Integrations in the island.";
    OauthBuiltInRejected => "Google sedang menolak login Vale — coba lagi nanti, atau pakai Client ID sendiri (Pengaturan → Integrasi → Lanjutan di island).",
        "Google is refusing Vale's sign-in right now — try again later, or use your own Client ID (Settings → Integrations → Advanced in the island).";
    OauthFailed => "Gagal terhubung — coba lagi dari island.",
        "Couldn't connect — try again from the island.";
    OauthScopes => "Izin Google belum lengkap — centang kedua izin, lalu coba lagi dari island.",
        "Google permissions incomplete — check both permissions, then try again from the island.";

    // Calendar page.
    Offline => "offline", "offline";
    ViewDay => "Hari", "Day";
    ViewWeek => "Minggu", "Week";
    ViewMonth => "Bulan", "Month";
    BackToTop => "Kembali ke atas", "Back to top";
    AddEvent => "Tambah acara", "Add event";
    ConnectGoogle => "Hubungkan Google", "Connect Google";
    ReconnectGoogle => "Hubungkan ulang Google", "Reconnect Google";
    AllDay => "Seharian", "All day";

    // Pomodoro page and compact view.
    Pomodoro => "Pomodoro", "Pomodoro";
    Focus => "Fokus", "Focus";
    Break => "Istirahat", "Break";
    LongBreak => "Istirahat panjang", "Long break";
    Sessions => "Jumlah sesi", "Sessions";
    MinutesShort => "mnt", "min";
    Start => "Mulai", "Start";
    Paused => "dijeda", "paused";
    Ready => "siap", "ready";
    ReadyClick => "Siap · klik", "Ready · click";
    NextUp => "Berikutnya:", "Next:";

    // Reminders page.
    Reminders => "Pengingat", "Reminders";
    RemindersEmpty => "Belum ada pengingat. Klik + Baru untuk menambah.",
        "No reminders yet. Click + New to add one.";
    DayToday => "Hari ini", "Today";
    PastDatesDimmed => "Tanggal lewat diredupkan", "Past dates are dimmed";
    DayTomorrow => "Besok", "Tomorrow";
    RepeatOnce => "Sekali", "Once";
    RepeatDaily => "Harian", "Daily";
    RepeatWeekly => "Mingguan", "Weekly";

    // Notes page.
    Notes => "Notes", "Notes";
    New => "Baru", "New";
    NotesEmpty => "Belum ada catatan. Klik + Baru untuk menulis.",
        "No notes yet. Click + New to write one.";
    Today => "hari ini", "Today";
    Yesterday => "kemarin", "Yesterday";

    // Media page.
    NothingPlaying => "Tidak ada musik yang diputar", "Nothing playing";
    NothingPlayingHint => "Putar lagu di aplikasi mana pun", "Play music in any app";
    LyricsButton => "Lirik", "Lyrics";
    NowPlaying => "Sedang diputar", "Now playing";

    // CLI page.
    Cli => "CLI", "CLI";
    CliEmpty => "Belum ada perintah", "No commands yet";
    CliEmptyHint => "Perintah jalan di folder ini; cd untuk pindah", "Commands run in this folder; cd to move";
    CliStop => "Stop", "Stop";
    CliHintReady => "Enter jalankan · ↑ riwayat · Esc keluar", "Enter runs · ↑ history · Esc leaves";
    CliHintRunning => "Ctrl+C berhenti", "Ctrl+C stops";
    CliHintSuggest => "Tab terima · ↑↓ pilih", "Tab accepts · ↑↓ choose";
    CliHintIdle => "Klik untuk mengetik", "Click to type";
    CliWaiting => "tunggu perintah selesai…", "waiting for the command to finish…";
    CliLocked => "Panel tetap terbuka saat mengetik · Esc untuk keluar",
        "The panel stays open while you type · Esc to leave";
    CliStopped => "dihentikan", "stopped";
    CliBusy => "Masih ada perintah berjalan — Ctrl+C untuk menghentikan",
        "A command is still running — Ctrl+C to stop it";
    // The CLI page's folder: the chip after an Explorer switch, and the menu.
    CliFromExplorer => "dari Explorer", "from Explorer";
    CliMenuExplorer => "Jendela Explorer terbuka", "Open Explorer windows";
    CliMenuFavorites => "Favorit", "Favorites";
    CliMenuRecent => "Terakhir dipakai di CLI", "Recently used in CLI";
    CliLastActive => "aktif terakhir", "last active";
    CliUnpinHint => "klik kanan = lepas", "right-click = unpin";
    CliMenuEmpty => "Belum ada folder — buka Explorer atau cd dulu",
        "No folders yet — open Explorer or cd first";
    CliJustNow => "baru saja", "just now";

    // System page and its notices.
    System => "Sistem", "System";
    SystemClean => "Bersihkan", "Clean up";
    SystemSoon => "segera", "soon";
    SystemBusiest => "Paling sibuk", "Busiest";
    SystemRam => "RAM", "RAM";
    SystemDiskBody => "Buka halaman Sistem untuk detailnya", "Open the System page for details";
    SystemRamBody => "Di atas batas lebih dari semenit", "Above the limit for over a minute";
    Open => "Buka", "Open";

    // The cleaner over the System page (deepclean-opsi-d.html).
    CleanModeOwn => "Pilih sendiri", "Pick yourself";
    CleanModeMole => "Otomatis · Mole", "Automatic · Mole";
    CleanTempLogs => "File sementara & log", "Temporary files & logs";
    CleanTempLogsSub => "Temp, log, laporan error > 7 hari", "Temp, logs, error reports > 7 days";
    CleanBrowser => "Cache browser", "Browser cache";
    CleanBrowserSub => "Chrome, Edge, Firefox, Brave, Opera · dilewati kalau browser terbuka",
        "Chrome, Edge, Firefox, Brave, Opera · skipped while the browser is open";
    CleanApps => "Cache aplikasi", "App cache";
    CleanAppsSub => "VS Code, Teams, Discord, Spotify, Zoom", "VS Code, Teams, Discord, Spotify, Zoom";
    CleanDev => "Cache developer", "Developer cache";
    CleanDevSub => "npm, pip, NuGet, Go, Cargo · diunduh ulang saat build",
        "npm, pip, NuGet, Go, Cargo · downloaded again on the next build";
    CleanAdobe => "Cache Autodesk & Adobe", "Autodesk & Adobe cache";
    CleanAdobeSub => "Dibuat ulang saat aplikasi dibuka", "Rebuilt when the app opens";
    CleanInstallers => "Installer lama di Downloads", "Old installers in Downloads";
    CleanInstallersSub => ".exe/.msi > 30 hari · ke Recycle Bin", ".exe/.msi > 30 days · to the Recycle Bin";
    CleanUpdate => "Sisa Windows Update", "Windows Update leftovers";
    CleanUpdateSub => "Perlu izin admin", "Needs admin permission";
    CleanAdminTag => "admin", "admin";
    CleanScanning => "memindai…", "scanning…";
    CleanBusy => "Membersihkan…", "Cleaning…";
    CleanCredit => "Ukuran dari pemindaian · daftar lokasi: Mole Windows (MIT)",
        "Sizes from the scan · location list: Mole Windows (MIT)";

    // The cleaner's "Otomatis · Mole" mode (deepclean-opsi-d.html d1, d2).
    MoleChip => "Eksperimental", "Experimental";
    MoleIntro => "Mole memilih sendiri apa yang dibersihkan · hapus permanen",
        "Mole picks what it cleans · deleted for good";
    MoleBrowserSub => "Chrome, Edge, Firefox, Brave, Opera", "Chrome, Edge, Firefox, Brave, Opera";
    MoleAppsSub => "VS Code, Teams, Discord, Zoom, Adobe, Autodesk, Steam",
        "VS Code, Teams, Discord, Zoom, Adobe, Autodesk, Steam";
    MoleDevSub => "npm, pip, NuGet, Go, Cargo, Docker, Git, IDE",
        "npm, pip, NuGet, Go, Cargo, Docker, Git, IDE";
    MoleTempSub => "Temp, log, prefetch, thumbnail, laporan error",
        "Temp, logs, prefetch, thumbnails, error reports";
    MoleOptSystem => "+ Sistem (admin)", "+ System (admin)";
    MoleOptGame => "+ Media game > 90 hari", "+ Game media > 90 days";
    MoleProtect => "Lindungi folder…", "Protect folders…";
    MoleProtectFailed => "Whitelist Mole tidak bisa ditulis: cache Cargo & Autodesk tidak terlindungi",
        "Could not write Mole's whitelist: the Cargo & Autodesk caches are not protected";
    MoleRun => "Jalankan Mole", "Run Mole";
    MoleConfirm => "Yakin? Hapus permanen", "Sure? Delete for good";
    MoleRunning => "Mole membersihkan…", "Mole is cleaning…";
    MoleAgain => "Pratinjau lagi", "Preview again";
    MoleScanning => "Memindai dengan Mole…", "Scanning with Mole…";
    MoleScanTime => "± 30 detik", "± 30 seconds";
    MoleSeeLog => "lihat log", "see log";
    MolePreviewFailed => "Pratinjau Mole gagal · lihat log", "Mole's preview failed · see log";
    MoleMissing => "Mole belum terpasang", "Mole is not installed";
    MoleMissingSub => "Mode otomatis butuh Mole Windows (eksperimental).",
        "Automatic mode needs Mole for Windows (experimental).";
    MoleHowTo => "Cara memasang \u{2197}", "How to install \u{2197}";
    MoleLog => "Log", "Log";
    MoleProtectSetting => "Lindungi cache Cargo & Autodesk di Mole",
        "Protect Cargo & Autodesk caches in Mole";
    MoleProtectSettingDesc => "Masuk whitelist Mole sekali, sebelum pratinjau pertama",
        "Added to Mole's whitelist once, before its first preview";

    // Shortcut page.
    Shortcuts => "Pintasan", "Shortcuts";
    // Settings → Panel → Pintasan.
    TileKindApp => "Aplikasi", "App";
    TileKindFolder => "Folder", "Folder";
    TileKindUrl => "Alamat web", "Web address";
    TileNotShown => "Tidak tampil (maks. 12)", "Not shown (12 max)";
    TilePageFull => "Halaman penuh (maks. 12)", "Page full (12 max)";
    ShortcutScreenshot => "Screenshot", "Screenshot";
    ShortcutClipboard => "Clipboard", "Clipboard";
    ShortcutSearch => "Cari", "Search";
    ShortcutEmoji => "Emoji", "Emoji";
    ShortcutExplorer => "Explorer", "Explorer";
    ShortcutCalculator => "Kalkulator", "Calculator";
    ShortcutTaskManager => "Pengelola Tugas", "Task Manager";
    ShortcutSettings => "Pengaturan Windows", "Windows Settings";
    ShortcutLock => "Kunci layar", "Lock screen";
    ShortcutGoShelf => "Rak", "Shelf";
    // Settings → Panel → Halaman panel and Pintasan: the lists.
    PagesShown => "Tampil di panel", "Shown on the panel";
    PagesHidden => "Tersembunyi", "Hidden";
    PagesLastShown => "Minimal satu halaman tampil", "At least one page must show";
    TilesShown => "Tampil di halaman", "Shown on the page";
    TilesLastShown => "Minimal satu pintasan tampil", "At least one shortcut must show";
    TilesAvailable => "Tersedia", "Available";
    TileGroupPages => "Halaman", "Pages";
    TileGroupWindows => "Windows", "Windows";
    TileGroupVale => "Vale", "Vale";
    TileGroupOwn => "Milik sendiri", "Your own";
    ListHide => "Sembunyikan", "Hide";
    ListShow => "Tampilkan", "Show";
    TileAdd => "+ Tambah", "+ Add";
    ListRestore => "Kembalikan bawaan", "Restore defaults";
    // The own-shortcut form.
    TileAddOwn => "+ Pintasan sendiri", "+ Your own shortcut";
    TileOwnDesc => "Aplikasi, folder, atau alamat web", "An app, a folder or a web address";
    TileOwnFull => "Sudah 8 pintasan sendiri (maks.)", "8 of your own already (max)";
    TileFormName => "Nama", "Name";
    TileFormKind => "Jenis", "Type";
    TileFormTarget => "Target", "Target";
    TileKindWeb => "Web", "Web";
    TileBrowse => "Pilih…", "Browse…";
    TileFormAdd => "Tambah", "Add";
    TileFilterApps => "Program (*.exe)", "Programs (*.exe)";
    TileFilterAll => "Semua berkas (*.*)", "All files (*.*)";
    TileErrName => "Nama harus 1 sampai 14 huruf, tanpa karakter kontrol",
        "The name takes 1 to 14 characters, no control characters";
    TileErrPath => "Isi jalur lengkap, misalnya C:\\Proyek (bukan jalur relatif atau jaringan)",
        "Give a full path such as C:\\Proyek (not a relative or network path)";
    TileErrUrl => "Alamat harus diawali http://, https:// atau mailto:",
        "The address must start with http://, https:// or mailto:";
    TileAddedOff => "Ditambahkan, belum tampil: halaman penuh (maks. 12)",
        "Added, not shown yet: the page is full (12 max)";

    // Editors.
    EditorNote => "Catatan", "Note";
    EditorEvent => "Acara", "Event";
    EditorAddEvent => "Tambah acara", "Add event";
    EditorEditEvent => "Edit acara", "Edit event";
    Delete => "Hapus", "Delete";
    Save => "Simpan", "Save";
    Cancel => "Batal", "Cancel";
    Close => "Tutup", "Close";
    Saving => "Menyimpan…", "Saving…";
    TitleRequired => "Judul wajib diisi", "Title is required";
    EndBeforeStart => "Jam selesai harus setelah jam mulai", "End time must be after start time";
    EditorNewReminder => "Pengingat baru", "New reminder";
    EditorEditReminder => "Edit pengingat", "Edit reminder";
    TextRequired => "Teks wajib diisi", "Text is required";
    TimePassed => "Waktunya sudah lewat", "That time has passed";
    NextWeek => "Minggu depan", "Next week";
    TitlePlaceholder => "Judul acara", "Event title";
    NotesPlaceholder => "Tambah catatan", "Add notes";
    NotePlaceholder => "Tulis catatan…", "Write a note…";
    ReminderPlaceholder => "Ingatkan saya untuk…", "Remind me to…";
    TypeTime => "Ketik jam", "Type a time";
    AllDayToggle => "Sepanjang hari", "All day";
    SavedOnThisComputer => "Disimpan di komputer ini", "Saved on this computer";
    // The reminder's time list, beside 08:00, 12:00, 17:00 and 20:00.
    PeriodMorning => "pagi", "morning";
    PeriodNoon => "siang", "noon";
    PeriodAfternoon => "sore", "afternoon";
    PeriodEvening => "malam", "evening";
    // The reminder's quick-time pills.
    QuickHalfHour => "30 mnt lagi", "In 30 min";
    QuickHour => "1 jam lagi", "In 1 hour";
    QuickTonight => "Nanti malam", "Tonight";
    QuickTomorrowMorning => "Besok pagi", "Tomorrow morning";
    ReminderPickHint => "Pilih tanggal dan jam, atau pakai pintasan.",
        "Pick a date and time, or use a shortcut.";
    // The weekly reminder's day chips, Monday first.
    DayLetterMon => "S", "M";
    DayLetterTue => "S", "T";
    DayLetterWed => "R", "W";
    DayLetterThu => "K", "T";
    DayLetterFri => "J", "F";
    DayLetterSat => "S", "S";
    DayLetterSun => "M", "S";

    // Tray menu.
    MenuStylePill => "Gaya: Pill", "Style: Pill";
    MenuStyleNotch => "Gaya: Notch", "Style: Notch";
    MenuExit => "Keluar", "Quit";
    MenuSettings => "Pengaturan…", "Settings…";
    MenuSearch => "Cari…", "Search…";
    MenuCheckUpdate => "Cek pembaruan…", "Check for updates…";

    // Self-update (Settings → Tentang, notices, `update.rs`).
    UpdateAuto => "Cek pembaruan otomatis", "Check for updates automatically";
    UpdateAutoDesc => "Sekali sehari dari GitHub", "Once a day from GitHub";
    UpdateCheckNow => "Cek sekarang", "Check now";
    UpdateChecking => "Memeriksa…", "Checking…";
    UpdateButton => "Update", "Update";
    UpdateLater => "Nanti", "Later";
    UpdateDebugOff => "Cek pembaruan mati di build debug", "Update check is off in debug builds";
    NotValeFile => "Ini bukan file pengaturan Vale.", "This is not a Vale settings file.";
    StopRecordingFirst => "Hentikan rekaman dulu", "Stop the recording first";
    SettingsImportedRestart => "Pengaturan diimpor. Mulai ulang Vale supaya semuanya berlaku.",
        "Settings imported. Restart Vale to apply everything.";
    BackupFailed => "Cadangan pengaturan lama gagal dibuat, jadi tidak ada yang diimpor.",
        "Could not back up the current settings, so nothing was imported.";
    ConfigFilterJson => "File JSON (*.json)", "JSON files (*.json)";
    UpdateErrNoNetwork => "tidak ada koneksi", "no connection";
    UpdateErrNotWritable => "folder tidak bisa ditulis", "folder is not writable";
    UpdateErrChecksum => "checksum tidak cocok", "checksum does not match";
    UpdateErrAsset => "file rilis tidak ada", "release file is missing";
    UpdateErrBadRelease => "rilis tidak terbaca", "release could not be read";

    // Settings window.
    SettingsTitle => "Pengaturan Vale", "Vale Settings";
    SecAppearance => "Tampilan", "Appearance";
    SecGeneral => "Umum", "General";
    SecPomodoro => "Pomodoro", "Pomodoro";
    SecIntegrations => "Integrasi", "Integrations";
    SecAbout => "Tentang", "About";
    PreviewTrack => "Sedang diputar", "Now playing";
    IslandStyle => "Gaya island", "Island style";
    IslandStyleDesc => "Bentuk island di atas layar", "Its shape at the top of the screen";
    StylePill => "Pill", "Pill";
    StyleNotch => "Notch", "Notch";
    AccentColor => "Warna aksen", "Accent color";
    AccentTerracotta => "Terakota", "Terracotta";
    AccentSage => "Sage", "Sage";
    AccentOchre => "Oker", "Ochre";
    AccentClay => "Lempung", "Clay";
    AccentOlive => "Zaitun", "Olive";
    AccentPlum => "Plum", "Plum";
    IslandSurface => "Warna dasar", "Base color";
    IslandSurfaceDesc => "Warna dasar island", "The island's base color";
    NotchWidth => "Lebar notch", "Notch width";
    NotchHeight => "Tinggi notch", "Notch height";
    NotchDefaultSize => "Ukuran bawaan", "Default size";
    NotchReset => "Atur ulang", "Reset";
    NotchFill => "Isi notch", "Notch fill";
    NotchFillDesc => "Panel tetap solid", "The open panel stays solid";
    NotchFillSolid => "Solid", "Solid";
    NotchFillTransparent => "Tembus", "See-through";
    NotchOpacity => "Opasitas", "Opacity";
    SurfaceEspresso => "Espresso", "Espresso";
    SurfaceGraphite => "Grafit", "Graphite";
    WindowMode => "Mode jendela", "Window mode";
    WindowModeDesc => "Pengaturan dan editor", "Settings and editors";
    ModeSystem => "Ikuti Windows", "Follow Windows";
    ModeDark => "Gelap", "Dark";
    ModeLight => "Terang", "Light";
    StayBehind => "Di belakang jendela", "Stay behind windows";
    StayBehindDesc => "Eksperimen: tertutup jendela, naik di tepi atas layar",
        "Experimental: covered by windows, rises at the top edge";
    IdleClock => "Jam saat kosong", "Clock when idle";
    IdleClockDesc => "Tampilkan jam saat island kosong", "Show the time when the island is idle";
    UiFont => "Font", "Font";
    UiFontDesc => "Island, panel, pengaturan", "Island, panel, settings";
    UiFontSans => "DM Sans", "DM Sans";
    UiFontMono => "JetBrains Mono", "JetBrains Mono";
    EqFollowAudio => "Equalizer ikut irama", "Equalizer follows the music";
    EqFollowAudioDesc => "Bar bergerak mengikuti suara yang diputar",
        "Bars move with the sound that is playing";
    PanelPages => "Halaman panel", "Panel pages";
    PageCalendar => "Kalender", "Calendar";
    PagePomodoro => "Pomodoro", "Pomodoro";
    PageReminders => "Pengingat", "Reminders";
    PageNotes => "Catatan", "Notes";
    PageMedia => "Musik", "Music";
    PageMixer => "Mixer", "Mixer";
    PageClipboard => "Clipboard", "Clipboard";
    PageNotifications => "Notifikasi", "Notifications";
    MixerMaster => "Volume utama", "Master volume";
    MixerSystemSounds => "Suara sistem", "System sounds";
    MixerEmpty => "Belum ada aplikasi yang bersuara", "No app is playing sound";
    MixerUnavailable => "Tidak ada perangkat suara", "No sound device";
    PageShelf => "Rak berkas", "File shelf";
    ShelfClear => "Kosongkan", "Clear";
    ShelfMissing => "hilang", "missing";
    ShelfDropHere => "Jatuhkan di sini", "Drop here";
    ShelfEmpty => "Seret berkas ke notch, ambil lagi nanti",
        "Drag files to the notch, take them out later";
    ShelfPhone => "HP", "Phone";
    ShelfFromPhone => "dari HP", "phone";
    PhoneStarting => "Mencari jaringan\u{2026}", "Finding the network\u{2026}";
    PhoneStep1 => "Scan pakai kamera HP", "Scan with the phone's camera";
    PhoneStep1Sub => "iPhone: Kamera \u{b7} Android: Kamera / Google Lens",
        "iPhone: Camera \u{b7} Android: Camera / Google Lens";
    PhoneStep2 => "Kirim atau ambil berkas di HP", "Send or take files on the phone";
    PhoneStep2Sub => "Semua berkas di rak ikut tampil", "Every shelf file shows there";
    PhoneNetPrivate => "pribadi", "private";
    PhoneNetPublic => "publik", "public";
    PhoneNetWifi => "Wi-Fi", "Wi-Fi";
    PhoneNetTailscale => "Tailscale", "Tailscale";
    PhoneTailscaleHint => "HP juga harus pakai Tailscale, akun yang sama",
        "The phone needs Tailscale too, same account";
    PhoneTailscaleOnly => "Lewat Tailscale, terenkripsi dari HP ke PC",
        "Through Tailscale, encrypted from phone to PC";
    PhoneConnected => "Tersambung", "Connected";
    PhonePublicHint => "Wi-Fi publik \u{2014} jangan dipakai di kafe/hotel", "Public Wi-Fi \u{2014} don't use at a caf\u{e9} or hotel";
    PhoneLocalOnly => "Langsung lewat Wi-Fi lokal, tidak lewat internet",
        "Straight over local Wi-Fi, not through the internet";
    PhoneFirewall => "Kalau Windows tanya, pilih Jaringan pribadi",
        "If Windows asks, choose Private network";
    PhoneNewQr => "Buat QR baru", "New QR code";
    PhoneQrExpired => "QR habis", "QR expired";
    PhoneWarnNoLan => "Tidak ada jaringan lokal", "No local network";
    PhoneWarnNoLanBody => "Sambungkan PC ke Wi-Fi atau kabel jaringan, lalu coba lagi.",
        "Connect the PC to Wi-Fi or a network cable, then try again.";
    PhoneWarnNoTailscale => "Tailscale belum jalan", "Tailscale isn't running";
    PhoneWarnNoTailscaleBody => "Nyalakan Tailscale di PC ini dan login, lalu coba lagi.",
        "Turn on Tailscale on this PC and sign in, then try again.";
    PhoneWarnBusy => "Port sedang dipakai", "Ports are in use";
    PhoneWarnBusyBody => "Port 47390 sampai 47399 dipakai program lain. Tutup programnya, lalu coba lagi.",
        "Ports 47390 to 47399 are used by other programs. Close them, then try again.";
    PhoneWarnFailed => "Tidak bisa memulai", "Could not start";
    PhoneWarnFailedBody => "Server lokal gagal menyala. Coba lagi sebentar lagi.",
        "The local server failed to start. Try again in a moment.";
    PhoneRetry => "Coba lagi", "Try again";
    PhoneReceiving => "Menerima dari HP", "From phone";
    PhonePickOnPhone => "Pilih berkas di HP", "Choose files on the phone";
    PhoneRowDone => "\u{2713} selesai", "\u{2713} done";
    PhoneRowCancelled => "dibatalkan", "cancelled";
    PhoneRowFailed => "gagal", "failed";
    PhoneCancelAll => "Batal", "Cancel";
    PhoneFinish => "Selesai", "Done";
    PhoneKindAndroid => "Android", "Android";
    PhoneKindIPhone => "iPhone", "iPhone";
    PhoneKindIPad => "iPad", "iPad";
    PhoneKindOther => "HP", "Phone";
    PageSystem => "Sistem", "System";
    PageDevices => "Perangkat", "Devices";
    PageArrange => "Susun", "Arrange";
    PageRecord => "Rekam", "Record";
    RecPause => "Jeda", "Pause";
    RecResume => "Lanjut", "Resume";
    RecRecording => "Merekam", "Recording";
    RecSourceMic => "Mic", "Mic";
    RecSourceSystem => "Suara PC", "PC audio";
    RecSourceBoth => "Dua-duanya", "Both";
    RecOpenFolder => "Buka folder", "Open folder";
    HotkeyRowRecord => "Rekam audio", "Record audio";
    HotkeyRowRecordDesc => "Mulai atau hentikan rekaman", "Start or stop a recording";
    HotkeyRowRecordMute => "Bisukan mic rekaman", "Mute recording mic";
    HotkeyRowRecordMuteDesc => "Hanya aktif saat merekam", "Active only while recording";
    HotkeyRowRecordScreen => "Rekam layar", "Record the screen";
    HotkeyRowRecordScreenDesc => "Pilih layar, jendela atau area", "Pick a screen, window or area";
    HotkeyRowRecordZoom => "Zoom video rekaman", "Zoom the recording video";
    HotkeyRowRecordZoomDesc => "Hanya aktif saat merekam layar", "Only while recording the screen";
    ScreenPickScreen => "Layar", "Screen";
    ScreenPickWindow => "Jendela", "Window";
    ScreenPickArea => "Area", "Area";
    ScreenPickRecord => "Rekam", "Record";
    ScreenPickHintScreen => "Klik layar yang mau direkam", "Click the screen to record";
    ScreenPickHintWindow => "Klik jendela yang mau direkam", "Click the window to record";
    ScreenPickHintArea => "Tarik kotak, atau klik jendela atau bagiannya", "Drag a box, or click a window or a part of it";
    RecNoticeSource => "Rekaman", "Recording";
    RecSavedTitle => "Rekaman tersimpan", "Recording saved";
    RecClickToOpen => "klik untuk buka", "click to open";
    TrNoticeSource => "Transkrip", "Transcript";
    TrStarting => "Bikin transkrip", "Making a transcript";
    TrReady => "Transkrip siap", "Transcript ready";
    TrFailedTitle => "Transkrip gagal", "Transcript failed";
    TrChipTranscribe => "Transkrip", "Transcribe";
    TrChipSettings => "Pengaturan", "Settings";
    TrWantOne => "Bikin transkrip?", "Want a transcript?";
    TrOffTitle => "Transkrip belum nyala", "Transcripts are off";
    TrOffBody => "Nyalakan di Pengaturan", "Turn them on in Settings";
    TrModelReady => "Model transkrip siap", "Transcript model ready";
    TrDownloadFailed => "Unduhan model gagal", "Model download failed";
    TrMadeNote => "Dijadikan catatan", "Saved as a note";
    TrWhyNoNetwork => "Tidak ada jaringan", "No network";
    TrWhyRefused => "Server menolak unduhan ({code})", "The server refused the download ({code})";
    TrWhyChecksum => "File unduhan rusak, coba lagi", "The download came out damaged, try again";
    TrWhyDiskFull => "Disk penuh", "The disk is full";
    TrWhyDisk => "File tidak bisa ditulis", "The file cannot be written";
    TrWhyArchive => "Paket mesin tidak cocok", "The engine package does not match";
    TrWhyRuntime => "Butuh Microsoft Visual C++ Redistributable (x64). Pasang dari situs Microsoft, lalu coba lagi.",
        "Needs the Microsoft Visual C++ Redistributable (x64). Install it from Microsoft's site, then try again.";
    TrWhyAudio => "File audio tidak bisa dibaca", "The audio file cannot be read";
    TrWhyEngine => "Mesin transkrip rusak. Hapus modelnya lalu unduh lagi di Pengaturan",
        "The transcript engine is damaged. Delete the model and download it again in Settings";
    TrWhyNoDevice => "Mesin transkrip tidak bisa jalan di PC ini",
        "The transcript engine cannot run on this PC";
    TrWhyTooLong => "Rekaman terlalu panjang untuk model ini",
        "The recording is too long for this model";
    TrWhyWrite => "Transkrip tidak bisa disimpan di folder ini",
        "The transcript cannot be saved in this folder";
    TrWhyOther => "Mesin transkrip gagal. Coba lagi", "The transcript engine failed. Try again";
    TrPillTranscribe => "Transkrip", "Transcribe";
    TrPillQueued => "Antre", "Queued";
    TrPillWaiting => "Menunggu model", "Waiting for model";
    TrPillDone => "Lihat", "View";
    TrPillFailed => "Gagal", "Failed";
    TrMenuView => "Lihat transkrip", "View transcript";
    TrMenuMake => "Bikin transkrip", "Make a transcript";
    TrMenuRedo => "Transkrip ulang", "Transcribe again";
    TrMenuCancel => "Batalkan transkrip", "Cancel the transcript";
    TrPickFile => "Transkrip file lain\u{2026}", "Transcribe a file\u{2026}";
    TrFilter => "Rekaman (*.m4a, *.wav, *.mp4)", "Recordings (*.m4a, *.wav, *.mp4)";
    RecFailedTitle => "Rekaman gagal", "Recording failed";
    RecMicDeniedTitle => "Mic diblokir Windows", "Windows blocked the mic";
    RecMicDeniedBody => "Klik untuk buka pengaturan mic", "Click to open the mic settings";
    RecMicLost => "Mic terputus", "Mic disconnected";
    RecSystemLost => "Suara PC terputus", "PC audio disconnected";
    RecKeepsGoing => "Rekaman tetap berjalan", "Recording continues";
    RecFolderBlockedTitle => "Folder rekaman diblokir Windows",
        "Windows blocked the recordings folder";
    RecFolderBlockedBody => "Klik untuk pilih folder lain", "Click to choose another folder";
    RecStoppedNoInput => "Rekaman dihentikan: tidak ada suara masuk",
        "Recording stopped: no audio input";
    RecUnfinished => "belum selesai", "unfinished";
    RecWhyNoMf => "Media Foundation tidak ada di Windows ini",
        "Media Foundation is missing on this Windows";
    RecWhyNoDevice => "Mic atau speaker tidak ditemukan", "No microphone or speaker found";
    RecWhyDiskFull => "Disk penuh", "The disk is full";
    RecWhyFolderGone => "Folder rekaman tidak ditemukan", "The recordings folder is gone";
    RecWhyNoAudio => "Tidak ada suara masuk", "No audio came in";
    RecNotSavedTitle => "Rekaman tidak disimpan", "Recording not saved";
    RecFallbackTitle => "Windows melindungi folder rekaman",
        "Windows protects the recordings folder";
    RecSavingTo => "Disimpan ke", "Saving to";
    RecChangeInSettings => "ganti di Pengaturan", "change it in Settings";
    RecWhyUnfinished => "File belum diberi nama \u{b7} klik untuk lihat",
        "The file kept its temporary name \u{b7} click to show";
    RecWhyOther => "Ada yang tidak beres saat merekam", "Something went wrong while recording";
    RecConsent => "Ingat: minta izin peserta sebelum merekam",
        "Remember to ask everyone before recording";
    TrayRecStart => "Mulai rekam", "Start recording";
    TrayRecStop => "Hentikan rekam", "Stop recording";
    TrayMicMute => "Bisukan mic", "Mute mic";
    TrayMicUnmute => "Nyalakan mic", "Unmute mic";
    TrayScreenStart => "Rekam layar", "Record the screen";
    TrayScreenStop => "Hentikan rekam layar", "Stop screen recording";
    RecSettingsTitle => "Rekaman", "Recording";
    RecSettingsSources => "Sumber suara", "Sound sources";
    RecSettingsSourcesDesc => "Yang ikut direkam", "What gets recorded";
    RecSettingsFolder => "Folder rekaman", "Recordings folder";
    RecSettingsFolderSubs => "Audio & Video", "Audio & Video";
    RecSettingsOpen => "Buka", "Open";
    RecSettingsChange => "Ganti", "Change";
    RecSettingsPickTitle => "Pilih folder rekaman", "Choose the recordings folder";
    ScreenSettingsTitle => "Rekam layar", "Screen recording";
    ScreenSettingsQuality => "Kualitas video", "Video quality";
    ScreenSettingsQualityOriginal => "Asli", "Original";
    ScreenSettingsFps => "Gambar per detik", "Frames per second";
    ScreenSettingsFpsDesc => "60 lebih halus, filenya lebih besar", "60 is smoother, the files are bigger";
    ScreenSettingsHighlight => "Sorot kursor", "Highlight the cursor";
    ScreenSettingsHighlightDesc => "Lingkaran kuning di kursor \u{b7} Berlaku mulai rekaman berikutnya", "A yellow disc on the cursor \u{b7} Applies from the next recording";
    ScreenSettingsRipples => "Riak klik", "Click ripples";
    ScreenSettingsRipplesDesc => "Riak melebar tiap klik \u{b7} Berlaku mulai rekaman berikutnya", "A ripple on every click \u{b7} Applies from the next recording";
    RecDeviceFrom => "Suara PC dari", "PC audio from";
    RecEchoCancel => "Peredam gema mic", "Mic echo cancellation";
    RecEchoCancelDesc => "Buang suara speaker \u{b7} Berlaku mulai rekaman berikutnya", "Removes speaker sound \u{b7} Applies from the next recording";
    RecFollowWindows => "Ikuti Windows", "Follow Windows";
    RecDeviceGone => "Perangkat tidak ada", "Device not found";
    RecMicSwitched => "Mic pindah", "Mic switched";
    RecSystemSwitched => "Suara PC pindah", "PC audio switched";
    RecMicMissing => "Mic pilihan tidak ada", "Chosen mic not found";
    RecSystemMissing => "Suara PC pilihan tidak ada", "Chosen PC audio not found";
    // The Rekam page's device line: the row's own name says which source.
    RecDeviceMissing => "Tidak ditemukan", "Not found";
    RecUsing => "Pakai", "Using";
    RecMicWord => "Mic", "Mic";
    RecSystemWord => "Suara PC", "PC audio";
    RecNotFound => "tidak ada", "not found";
    RecEmpty => "Belum ada rekaman", "No recordings yet";
    RecBusyTitle => "Rekaman lain sedang jalan", "Another recording is running";
    RecBusyBody => "Hentikan dulu, lalu mulai lagi", "Stop it first, then start again";
    ScreenSavedTitle => "Video tersimpan", "Video saved";
    ScreenWindowClosedTitle => "Jendela ditutup, video tersimpan", "Window closed, video saved";
    ScreenDisplayGoneTitle => "Layar terputus, video tersimpan",
        "Display disconnected, video saved";
    ScreenDiscardedTitle => "Video dibuang", "Video discarded";
    ScreenFailedTitle => "Rekam layar gagal", "Screen recording failed";
    ScreenNoSoundTitle => "Video jalan tanpa suara", "Recording video without sound";
    ScreenWhyUnsupported => "Windows ini tidak bisa merekam layar",
        "This Windows cannot record the screen";
    ScreenWhyGone => "Jendela atau layarnya sudah tidak ada", "The window or screen is gone";
    ScreenWhyMinimized => "Jendelanya sedang diminimalkan. Buka dulu, lalu coba lagi",
        "The window is minimized. Restore it, then try again";
    ScreenWhyEncoder => "Pengode video tidak tersedia di PC ini",
        "No video encoder is available on this PC";
    ScreenWhyGpu => "Kartu grafis tidak bisa dipakai untuk merekam",
        "The graphics card cannot be used to record";
    ScreenWhyWrite => "File video tidak bisa ditulis", "The video file cannot be written";
    ScreenWhyOther => "Ada yang tidak beres saat merekam layar",
        "Something went wrong while recording the screen";
    ScreenLabel => "Layar", "Screen";
    ScreenCountdownEsc => "Esc untuk batal", "Esc to cancel";
    RecTabAudio => "Audio", "Audio";
    RecTabScreen => "Layar", "Screen";
    ScreenIdleHint => "Layar, jendela atau area: dipilih setelah tekan Rekam",
        "Screen, window or area: you pick it after Record";
    ScreenDiscard => "Buang", "Discard";
    RecRecordLabel => "Rekam", "Record";
    RecNow => "Sekarang", "Now";
    RecMuted => "Dibisukan", "Muted";
    ScreenDiscardAsk => "Buang video ini?", "Throw this video away?";
    ScreenDiscardKeep => "Simpan", "Keep";
    RecQuality => "Kualitas", "Quality";
    ScreenNoVideos => "Belum ada video", "No videos yet";
    ArrangeSkip => "Lewati ubin", "Skip tile";
    ArrangeSaved => "Tersimpan", "Saved";
    ArrangeHint => "Backspace kembali \u{b7} Esc batal", "Backspace back \u{b7} Esc cancel";
    ArrangeNoWindows => "Tidak ada jendela yang bisa dipilih", "No windows to pick";
    ArrangeNameHint => "Nama susunan \u{b7} Enter simpan \u{b7} Esc batal",
        "Name \u{b7} Enter to save \u{b7} Esc to cancel";
    ArrangeSaveAs => "Simpan\u{2026}", "Save\u{2026}";
    ArrangeUndo => "Batalkan", "Undo";
    ArrangeFull => "Daftar penuh (maks. 12)", "List is full (max 12)";
    ArrangeDefaultName => "Susunan", "Arrangement";
    ArrangeCols2 => "2 kolom", "2 columns";
    ArrangeCols3 => "3 kolom", "3 columns";
    ArrangeMain2 => "Utama + 2", "Main + 2";
    ArrangeGrid4 => "Grid 2\u{d7}2", "2\u{d7}2 grid";
    ArrangeTwoThirds => "2/3 + 1/3", "2/3 + 1/3";
    ArrangeMain3 => "Utama + 3", "Main + 3";
    ArrangeGap => "Jarak antar ubin", "Gap between tiles";
    ArrangeGapDesc => "Dalam piksel, dihitung per monitor", "In pixels, scaled per monitor";
    ArrangeMyTemplates => "Template saya", "My templates";
    ArrangeNewTemplate => "+ Template baru", "+ New template";
    ArrangeTemplatesFull => "Penuh (maks. 12)", "Full (max 12)";
    ArrangeNoTemplates => "Belum ada template", "No templates yet";
    ArrangeSavedList => "Susunan tersimpan", "Saved arrangements";
    ArrangeNoSaved => "Belum ada susunan tersimpan", "No saved arrangements yet";
    ArrangeEdit => "Ubah", "Edit";
    ArrangeDelete => "Hapus", "Delete";
    ArrangeRename => "Ganti nama", "Rename";
    ArrangeSplitCols => "Belah kiri-kanan", "Split left-right";
    ArrangeSplitRows => "Belah atas-bawah", "Split top-bottom";
    ArrangeMerge => "Gabung", "Merge";
    ArrangeFieldName => "Nama", "Name";
    ArrangeSaveButton => "Simpan", "Save";
    ArrangeCancel => "Batal", "Cancel";
    ArrangeEditorHint => "Klik ubin, belah, atau seret pembagi",
        "Click a tile, split it, or drag a divider";
    DevicesEject => "Eject", "Eject";
    DevicesEjecting => "Melepas…", "Ejecting…";
    DevicesEjectAll => "Eject semua", "Eject all";
    DevicesRetry => "Coba lagi", "Try again";
    DevicesEmpty => "Tidak ada flashdisk atau disk eksternal", "No removable drives";
    DeviceModel => "Model", "Model";
    DeviceConnection => "Jalur", "Connection";
    DeviceFileSystem => "Sistem file", "File system";
    DeviceCardReader => "Pembaca kartu", "Card reader";
    DeviceBusOther => "Lainnya", "Other";
    DeviceKindUsb => "Flashdisk", "Flash drive";
    DeviceKindHdd => "Disk eksternal", "External disk";
    DeviceKindSd => "Kartu SD", "SD card";
    DeviceSafe => "Aman dicabut", "Safe to remove";
    VetoTitle => "Belum bisa dilepas.", "Can't remove it yet.";
    VetoInUse => "Masih dipakai aplikasi lain.", "Still in use by another app.";
    VetoService => "Masih dipakai layanan Windows.", "In use by a Windows service.";
    VetoDevice => "Perangkat lain masih memakainya.", "Another device is still using it.";
    VetoDriver => "Driver-nya menolak dilepas.", "Its driver refused.";
    VetoNotAllowed => "Windows tidak mengizinkan perangkat ini dilepas.",
        "Windows does not allow removing this device.";
    VetoRights => "Butuh izin administrator.", "Needs administrator rights.";
    VetoPower => "Dayanya tidak cukup.", "Not enough power.";
    VetoRefused => "Windows menolak melepasnya.", "Windows refused to remove it.";
    DeviceNotices => "Notifikasi perangkat", "Device notifications";
    DeviceNoticesDesc => "Saat flashdisk dicolok dan saat aman dicabut",
        "When a drive is plugged in and when it is safe to remove";
    PageCli => "CLI", "CLI";
    PageShortcuts => "Pintasan", "Shortcuts";
    Language => "Bahasa", "Language";
    LanguageDesc => "Island dan semua jendela", "The island and every window";
    LangIndonesian => "Indonesia", "Indonesia";
    LangEnglish => "English", "English";
    Autostart => "Jalan saat Windows mulai", "Start with Windows";
    AutostartDesc => "Buka island otomatis saat masuk", "Open the island when you sign in";
    BatteryAlert => "Peringatan baterai", "Battery alert";
    BatteryAlertDesc => "Kasih tahu saat baterai di bawah ini", "Warn when the charge drops below";
    NoBattery => "Tidak ada baterai", "No battery found";
    DiskAlert => "Peringatan disk", "Disk alert";
    DiskAlertDesc => "Saat sisa ruang di bawah", "When free space drops below";
    RamAlert => "Peringatan RAM", "RAM alert";
    RamAlertDesc => "Di atas, selama 1 menit", "Above it for 1 minute";
    PopupsHelp => "Cara matikan popup Windows", "Turn off Windows pop-ups";
    PopupsHelpDesc => "Supaya notifikasi tidak muncul dua kali", "So notifications don't show twice";
    ShowHow => "Buka petunjuk", "Show me how";
    LyricsInNotch => "Lirik di notch", "Lyrics in notch";
    LyricsInNotchDesc => "Artis, judul & durasi dikirim ke lrclib.net", "Artist, title & length are sent to lrclib.net";
    ShellCli => "Shell CLI", "CLI shell";
    ShellCliDesc => "Untuk perintah di halaman CLI", "Runs the CLI page's commands";
    ShellPowerShell => "PowerShell", "PowerShell";
    ShellCmd => "cmd", "cmd";
    ShotSaveFailed => "Gagal menyimpan", "Could not save";
    OpenModeSetting => "Mode buka", "Open mode";
    OpenModeDesc => "Cara panel dibuka", "How the panel opens";
    OpenModeHover => "Hover", "Hover";
    OpenModeStay => "Hover & tetap", "Hover & stay";
    OpenModeClick => "Klik", "Click";
    CliStartFolder => "Folder awal CLI", "CLI start folder";
    CliStartFolderDesc => "Saat halaman CLI dibuka", "When the CLI page opens";
    CliStartExplorer => "Ikuti Explorer", "Follow Explorer";
    CliStartLast => "Folder terakhir", "Last folder";
    FocusDesc => "Lama satu sesi fokus", "Length of one focus session";
    BreakDesc => "Jeda di antara sesi", "Pause between sessions";
    LongBreakDesc => "Setelah sesi terakhir", "After the last session";
    SessionsDesc => "Sesi fokus sebelum istirahat panjang", "Focus sessions before a long break";
    PomodoroLocked => "Pomodoro sedang berjalan — reset dulu untuk mengubah ini.",
        "A pomodoro is running — reset it to change these.";
    GoogleCalendar => "Google Calendar", "Google Calendar";
    StatusNotConfigured => "Belum diatur", "Not set up";
    StatusNeedsClientId => "Isi Client ID dulu", "Add a Client ID";
    StatusNeedsLogin => "Belum terhubung", "Not connected";
    StatusConnected => "Terhubung", "Connected";
    StatusReconnect => "Perlu login ulang", "Sign in again";
    StatusOffline => "Offline", "Offline";
    ClientId => "Client ID", "Client ID";
    ClientSecret => "Client secret", "Client secret";
    SetupGuide => "Panduan setup", "Setup guide";
    Connect => "Hubungkan", "Connect";
    Disconnect => "Putuskan", "Disconnect";
    ConfirmShort => "Yakin?", "Sure?";
    Saved => "Tersimpan", "Saved";
    CheckBrowser => "Cek browser…", "Check your browser…";
    Disconnected => "Terputus", "Disconnected";
    ClaudeCode => "Claude Code", "Claude Code";
    HooksOn => "Hook terpasang", "Hooks installed";
    HooksOff => "Hook belum terpasang", "Hooks not installed";
    HooksUnknown => "Status tidak diketahui", "Status unknown";
    InstallHooks => "Pasang hook", "Install hooks";
    RemoveHooks => "Hapus hook", "Remove hooks";
    HooksAlready => "Sudah terpasang", "Already installed";
    HooksRemoved => "Hook dihapus", "Hooks removed";
    HooksNone => "Tidak ada hook", "No hooks found";
    Failed => "Gagal", "Failed";
    AboutTagline => "Island ala macOS untuk Windows 11", "A macOS-style island for Windows 11";
    SourceCode => "Kode sumber", "Source code";
    OpenGithub => "Buka GitHub", "Open GitHub";
    Updates => "Pembaruan", "Updates";
    SeeReleases => "Lihat rilis", "See releases";
    DataFolder => "Folder data", "Data folder";
    SettingsImported => "Pengaturan diimpor", "Settings imported";
    OpenDataFolder => "Buka folder data", "Open data folder";

    // Vale's mascot, the notch assistant (Settings → Maskot).
    RobotInNotch => "Maskot di notch", "Mascot in the notch";
    RobotInNotchDesc => "Tampilkan maskot Vale di samping jam dan notice",
        "Show Vale's mascot beside the clock and notices";
    RobotMotion => "Animasi", "Animation";
    // The design's sentence; it wraps beside the three segments.
    RobotMotionDesc => "Hemat: tanpa kedip dan gerak, ekspresi tetap",
        "Saver: no blinking or motion, faces stay";
    MotionFull => "Penuh", "Full";
    MotionSaver => "Hemat", "Saver";
    MotionOff => "Mati", "Off";
    RobotSleep => "Tidur setelah", "Sleep after";
    RobotSleepDesc => "Tanpa input mouse atau keyboard di PC", "No mouse or keyboard input on the PC";
    MorningGreetingDesc => "Ringkasan acara dan pengingat hari ini",
        "Today's events and reminders";
    GoodMorning => "Selamat pagi", "Good morning";
    SourceVale => "Vale", "Vale";
    // The robot's faces, as the demo sheet labels them.
    ExprIdle => "Diam", "Idle";
    ExprHappy => "Senang", "Happy";
    ExprFocus => "Fokus", "Focus";
    ExprMusic => "Mendengar", "Listening";
    ExprSing => "Bernyanyi", "Singing";
    ExprAlert => "Waspada", "Alert";
    ExprThink => "Berpikir", "Thinking";
    ExprSleep => "Tidur", "Asleep";
    ExprSad => "Lesu", "Low";
    ExprWave => "Menyapa", "Waving";
    ExprTyping => "Mengetik", "Typing";
    ExprBlink => "Kedip", "Blink";

    // The Clipboard page, its notice and Settings → Clipboard.
    ClipboardTitle => "Clipboard", "Clipboard";
    ClipKindText => "Teks", "Text";
    ClipKindLink => "Tautan", "Link";
    ClipKindCode => "Kode", "Code";
    ClipKindColor => "Warna", "Color";
    ClipKindImage => "Gambar", "Image";
    ClipKindFiles => "File", "Files";
    ClipEmpty => "Belum ada yang disalin", "Nothing copied yet";
    ClipEmptyHint => "Salin sesuatu, nanti muncul di sini", "Copy something and it shows up here";
    ClipNoMatch => "Tidak ada yang cocok", "No matches";
    ClipOff => "Riwayat clipboard mati", "Clipboard history is off";
    ClipOffHint => "Nyalakan di Pengaturan → Clipboard", "Turn it on in Settings → Clipboard";
    ClipCopied => "Disalin", "Copied";
    // An image card dragged out (only said when the small preview went).
    ClipDragged => "Ditarik", "Dragged";
    // The copy preview (`NoticeKind::ClipPreview`): an image, files, the chip.
    ClipPreviewImage => "Gambar disalin", "Image copied";
    ClipPreviewFiles => "Berkas disalin", "Files copied";
    ClipPreviewChip => "Riwayat", "History";
    ClipImageSmall => "pratinjau kecil, aslinya tidak tersimpan",
        "small preview, the original is no longer kept";
    SecClipboard => "Clipboard", "Clipboard";
    ClipHistory => "Riwayat clipboard", "Clipboard history";
    ClipHistoryDesc => "Simpan yang kamu salin", "Keep what you copy";
    ClipItems => "Jumlah item", "Items kept";
    ClipItemsDesc => "Item tersemat tidak ikut terhapus", "Pinned items are never pushed out";
    ClipPersist => "Simpan setelah restart", "Keep after restart";
    ClipPersistDesc => "Terenkripsi DPAPI, hanya akun ini", "Encrypted with DPAPI, this account only";
    ClipClearOnLock => "Hapus saat layar dikunci", "Clear when the screen locks";
    ClipClearOnLockDesc => "Item tersemat tetap", "Pinned items stay";

    // Quick search.
    SearchTitle => "Pencarian", "Search";
    SearchHint => "Cari aplikasi, berkas, hitung, atau ketik ? untuk web",
        "Search apps, files, calculate, or type ? for the web";
    SearchBack => "Kembali", "Back";
    SearchEmpty => "Ketik untuk mencari", "Type to search";
    SearchEmptyHint => "Contoh: kalkulator · laporan · 2*(3+4) · ? kopi susu",
        "Try: calculator · report · 2*(3+4) · ? coffee";
    SearchNone => "Tidak ada hasil", "No results";
    SearchNoneHint => "Setelah ? tulis yang dicari di web", "After ? type what to look up on the web";
    SearchKindApp => "Aplikasi", "App";
    SearchKindFile => "Berkas", "File";
    SearchKindFolder => "Folder", "Folder";
    SearchIndexing => "Membaca berkas…", "Reading files…";
    SearchRootDesktop => "Desktop", "Desktop";
    SearchRootDocuments => "Dokumen", "Documents";
    SearchRootDownloads => "Unduhan", "Downloads";
    SearchRootPictures => "Gambar", "Pictures";
    SearchRootVideos => "Video", "Videos";
    SearchRootMusic => "Musik", "Music";
    SearchKindSetting => "Pengaturan Windows", "Windows Settings";
    SearchCopy => "Enter untuk menyalin", "Enter to copy";
    SearchKindWeb => "Cari di web", "Search the web";
    ClipFeedbackAll => "\u{201C}Disalin\u{201D} untuk semua salinan", "\u{201C}Copied\u{201D} for every copy";
    ClipFeedbackAllDesc => "Hanya saat pratinjau mati", "Only while the preview is off";
    ClipCopyNotice => "Pratinjau salinan", "Copy preview";
    ClipCopyNoticeDesc => "Pratinjau di notch setiap kali menyalin", "A preview in the notch on every copy";
    ClipFeedbackAllReplaced => "Diganti pratinjau salinan di atas", "Replaced by the copy preview above";
    ClipClear => "Hapus riwayat", "Clear history";
    ClipClearDesc => "Item tersemat tetap disimpan", "Pinned items are kept";
    ClipClearButton => "Hapus", "Clear";

    // The Notifikasi page and Settings → Fitur → Notifikasi.
    NotifJustNow => "baru saja", "just now";
    NotifYesterday => "Kemarin", "Yesterday";
    NotifPause => "Jeda", "Pause";
    NotifClearAll => "Hapus semua", "Clear all";
    NotifSure => "Yakin?", "Sure?";
    NotifOpen => "Buka", "Open";
    NotifMute => "Kecualikan app ini", "Exclude this app";
    NotifRemove => "Hapus dari Vale", "Remove from Vale";
    NotifOff => "Simpan notifikasi mati", "Saving notifications is off";
    NotifEmpty => "Belum ada notifikasi", "No notifications yet";
    NotifEmptyHint => "Notifikasi Windows yang masuk muncul di sini",
        "New Windows notifications show up here";
    NotifPaused => "Dijeda, notifikasi baru tidak disimpan",
        "Paused, new notifications are not kept";
    NotifNoAccess => "Izin notifikasi belum diberikan", "Notification access not granted";
    NotifOpenSettings => "Buka pengaturan", "Open settings";
    SecNotifications => "Notifikasi", "Notifications";
    NotifToastStyle => "Notifikasi saat notch dipakai", "Notifications on a busy notch";
    NotifToastStyleDesc => "Ringkas: strip atau logo kecil",
        "Compact: a strip or a small logo";
    NotifToastCompact => "Ringkas", "Compact";
    NotifToastFull => "Penuh", "Full";
    NotifRecord => "Simpan notifikasi", "Save notifications";
    NotifRecordDesc => "Riwayat notifikasi Windows di halaman Notifikasi",
        "Windows notifications, kept on the Notifications page";
    NotifMax => "Jumlah maksimal", "Most kept";
    NotifMaxDesc => "Yang paling lama keluar duluan", "The oldest go first";
    NotifPersist => "Simpan setelah restart", "Keep after restart";
    NotifPersistDesc => "Terenkripsi DPAPI, hanya akun ini",
        "Encrypted with DPAPI, this account only";
    NotifMuted => "Aplikasi dikecualikan", "Excluded apps";
    NotifMutedDesc => "Tidak muncul di notch maupun riwayat",
        "Shown in neither the notch nor the history";
    NotifMutedHint => "Nama aplikasi", "App name";
    NotifMutedAdd => "Tambah", "Add";
    NotifMutedNone => "Belum ada yang dikecualikan", "None excluded yet";
    NotifClearHistory => "Hapus riwayat notifikasi", "Clear notification history";
    NotifClearHistoryDesc => "Daftar notifikasi di Windows tidak disentuh",
        "Windows' own list is left alone";

    // The screenshot preview, its notices and Settings → Clipboard → Screenshot.
    ShotTitle => "Screenshot", "Screenshot";
    ShotCopiedNote => "sudah disalin", "already copied";
    ShotCopyAgain => "Salin lagi", "Copy again";
    ShotKeep => "Simpan", "Keep";
    ShotDelete => "Hapus", "Delete";
    ShotSaved => "Disimpan", "Saved";
    ShotAlreadySaved => "Sudah tersimpan", "Already saved";
    ShotNotSaved => "Belum ada file", "No file yet";
    ShotOnClipboard => "tetap di clipboard", "still on the clipboard";
    ShotClickToOpen => "klik untuk buka", "click to open";
    ShotMaybeBlocked => "mungkin diblokir Controlled folder access",
        "maybe blocked by Controlled folder access";
    ShotDeleted => "Dihapus", "Deleted";
    ShotDeleteFailed => "Gagal menghapus", "Couldn't delete";
    ShotClipEmptied => "clipboard dikosongkan", "clipboard cleared";
    ShotClipUnchanged => "clipboard tidak diubah", "clipboard unchanged";
    ShotRecycled => "dipindah ke Recycle Bin", "moved to the Recycle Bin";
    ShotNoFile => "tidak ada file", "no file found";
    ShotFileStays => "file masih ada", "the file is still there";
    SecScreenshot => "Screenshot", "Screenshot";
    ShotPreviewSetting => "Pratinjau di notch", "Preview in the notch";
    ShotIgnoreSetting => "Jika diabaikan", "If ignored";
    ShotIgnoreDesc => "Setelah waktu tampil habis", "Once its time on screen runs out";
    ShotShowFor => "Lama tampil", "Show for";
    SecondsShort => "dtk", "s";
    // Vale's own region capture and screenshot editor.
    ShotToolSetting => "Alat screenshot", "Screenshot tool";
    ShotToolVale => "Vale", "Vale";
    ShotToolWindows => "Snipping Tool", "Snipping Tool";
    ShotToolDesc => "Dipakai tile Screenshot", "Used by the Screenshot tile";
    ShotAfterCaptureSetting => "Setelah capture", "After capture";
    ShotAfterEditor => "Jendela editor", "Editor window";
    ShotAfterInline => "Langsung di layar", "On screen";
    // The brief's "Screenshot Vale: tile dan PrtSc" needs 202 px; the row
    // leaves 149 beside the two wide choices (`texts_fit_in_both_languages`).
    ShotAfterCaptureDesc => "Untuk tile dan PrtSc", "For tile and PrtSc";
    ShotAutoCopySetting => "Salin otomatis", "Copy automatically";
    ShotAutoCopyDesc => "Hanya screenshot Vale; Snipping Tool menyalin sendiri",
        "Vale's own capture only; Snipping Tool copies by itself";
    SnapHint => "Klik atau seret · Enter ambil · Esc batal",
        "Click or drag · Enter to capture · Esc to cancel";
    EditTitle => "Vale · Screenshot", "Vale · Screenshot";
    EditSelect => "Pilih", "Select";
    EditText => "Teks", "Text";
    EditRect => "Kotak", "Rectangle";
    EditLine => "Garis", "Line";
    InlineOpenEditor => "Buka di editor", "Open in editor";
    EditArrow => "Panah", "Arrow";
    EditPen => "Pena", "Pen";
    EditMarker => "Stabilo", "Marker";
    EditPixelate => "Sensor", "Pixelate";
    EditUndo => "Urungkan", "Undo";
    EditRedo => "Ulangi", "Redo";
    EditCopy => "Salin", "Copy";
    EditSave => "Simpan", "Save";
    EditClose => "Tutup", "Close";
    EditDiscardAsk => "Buang perubahan?", "Discard changes?";
    EditDiscard => "Buang", "Discard";
    EditKeep => "Batal", "Cancel";
    EditCopied => "Disalin ke clipboard", "Copied to the clipboard";
    EditCopyFailed => "Gagal menyalin", "Couldn't copy";
    EditAutoCopyFailed => "Gagal menyalin · tekan Salin", "Couldn't copy · press Copy";
    EditPreviewFailed => "Gambar terlalu besar untuk ditampilkan di sini",
        "The picture is too big to show here";
    EditSaving => "Menyimpan…", "Saving…";
    EditDemoNoClipboard => "Mode demo: clipboard tidak disentuh",
        "Demo mode: the clipboard is not touched";
    InlineCopyFailed => "Gagal menyalin, coba lagi", "Couldn't copy, try again";
    EditHint => "Seret untuk menggambar · Del untuk hapus · Ctrl+Z urungkan",
        "Drag to draw · Delete removes · Ctrl+Z undoes";
    EditStroke => "Tebal garis", "Stroke width";
    EditColor => "Warna", "Colour";

    // The demo's Claude notices (debug builds).
    DemoClaudeWorking => "Claude sedang bekerja", "Claude is working";
    DemoClaudeAsks => "izin menjalankan perintah", "permission to run a command";
    DemoFilesChanged => "3 file diubah", "3 files changed";

    // Settings, v3.8: the Maskot page (Kucing before), the new sections and
    // the cards' titles.
    SecCat => "Maskot", "Mascot";
    SecPanel => "Panel", "Panel";
    SecFeatures => "Fitur", "Features";
    // Settings → Tombol pintas: six hotkeys recorded by pressing them.
    SecHotkeys => "Tombol pintas", "Hotkeys";
    GroupHotkeys => "Di semua aplikasi", "In every app";
    HotkeysHint => "Klik kolom, lalu tekan tombolnya", "Click a field, then press the keys";
    HotkeysResetAll => "Kembalikan semua", "Reset all";
    HotkeyRowSearch => "Pencarian cepat", "Quick search";
    HotkeyRowSearchDesc => "Cari aplikasi, file, dan hitung", "Find apps and files, calculate";
    HotkeyRowClipboard => "Buka Clipboard", "Open Clipboard";
    HotkeyRowClipboardDesc => "Riwayat salinan di panel", "Your copy history in the panel";
    HotkeyRowDevices => "Buka Perangkat", "Open Devices";
    HotkeyRowDevicesDesc => "Flashdisk dan disk eksternal", "Flash drives and external disks";
    HotkeyRowArrange => "Buka Susun", "Open Arrange";
    HotkeyRowArrangeDesc => "Atur jendela jadi tata letak", "Arrange windows into a layout";
    HotkeyRowScreenshot => "Screenshot Vale", "Vale screenshot";
    HotkeyRowScreenshotDesc => "Ambil area layar, lalu edit", "Capture an area, then edit";
    HotkeyRowLyricsCard => "Buka lirik", "Open lyrics";
    HotkeyRowLyricsCardDesc => "Kartu lirik melayang", "Floating lyrics card";
    // The floating lyrics card's messages.
    LyricsCardSearching => "Mencari lirik…", "Searching for lyrics…";
    LyricsCardNone => "Lirik sinkron nggak ketemu buat lagu ini", "No synced lyrics found for this song";
    LyricsCardNoMedia => "Nggak ada lagu yang diputer", "Nothing is playing";
    HotkeyNone => "Tidak ada", "None";
    HotkeyRecording => "Tekan tombol… (Esc batal)", "Press keys… (Esc cancels)";
    HotkeyTaken => "Sudah dipakai aplikasi lain", "Taken by another app";
    HotkeyReserved => "Dipakai Windows", "Reserved by Windows";
    HotkeyNeedsModifier => "Tambahkan Ctrl, Alt, atau Win", "Add Ctrl, Alt or Win";
    GroupBasics => "Dasar", "Basics";
    GroupAlerts => "Peringatan", "Alerts";
    GroupIsland => "Island", "Island";
    GroupNotch => "Notch", "Notch";
    GroupWindow => "Jendela", "Window";
    GroupCat => "Maskot", "Mascot";
    GroupMotion => "Gerak", "Motion";
    GroupGreeting => "Sapaan", "Greeting";
    GroupBehavior => "Perilaku", "Behavior";
    GroupHistory => "Riwayat", "History";
    GroupPomodoro => "Pomodoro", "Pomodoro";
    GroupMusic => "Musik", "Music";
    GroupCli => "CLI", "CLI";
    GroupCleaner => "Pembersih", "Cleaner";
    GroupLinks => "Tautan", "Links";
    SurfaceAuto => "Ikuti maskot", "Match mascot";
    SurfaceSea => "Laut", "Sea";
    SurfaceNight => "Malam", "Night";
    SurfacePlum => "Ungu", "Plum";
    SurfaceTerminal => "Terminal", "Terminal";
    CatColorTerracotta => "Terakota", "Terracotta";
    CatColorCalico => "Calico", "Calico";
    CatColorWhite => "Putih", "White";
    CatColorBlack => "Hitam", "Black";
    CatColorLabel => "Warna", "Color";
    // Settings → Maskot: the mascot row, and the falk's and octopus's coats.
    MascotLabel => "Maskot", "Mascot";
    MascotCat => "Kucing", "Cat";
    MascotFalk => "Falk", "Cockatiel";
    MascotOctopus => "Gurita", "Octopus";
    FalkColorLutino => "Lutino", "Lutino";
    FalkColorGrey => "Abu-abu", "Grey";
    FalkColorWhiteFace => "Muka putih", "White face";
    OctopusColorCoral => "Koral", "Coral";
    OctopusColorRed => "Merah", "Red";
    OctopusColorPurple => "Ungu", "Purple";
    OctopusColorBlue => "Biru laut", "Sea blue";
    CatBackdrop => "Latar", "Backdrop";
    CatBackdropChange => "Ubah", "Change";
    CatAccessoryLabel => "Aksesori", "Accessory";
    AccNone => "Tanpa", "None";
    AccGlasses => "Kacamata", "Glasses";
    AccBow => "Pita", "Bow";
    AccNightcap => "Topi tidur", "Nightcap";
    AccScarf => "Syal", "Scarf";
    AccFlower => "Bunga", "Flower";
    AccCrown => "Mahkota", "Crown";
    AccHeadphone => "Headphone", "Headphones";
    AccSailor => "Topi pelaut", "Sailor cap";
    AccBowtie => "Dasi kupu-kupu", "Bow tie";
    CatFollow => "Ikuti kursor", "Follow the pointer";
    CatFollowDesc => "Kepala dan mata menoleh ke kursor", "Head and eyes turn toward the pointer";
    CatPeek => "Mengecil jadi mata", "Shrink to eyes";
    CatPeekDesc => "Jadi dua mata saat kursor mendekat di atas jendela",
        "Two eyes when the pointer nears it over a window";
    CatPeekBehind => "Mati selama \u{201C}Di belakang jendela\u{201D} menyala",
        "Off while \u{201C}Stay behind windows\u{201D} is on";
    CatDance => "Joget ikut musik", "Dance to music";
    CatDanceDesc => "Mengangguk ikut irama saat musik diputar", "Nods to the beat while music plays";
    CatGreeting => "Sapaan", "Greeting";

    // Monitor choice (Settings -> Tampilan -> Notch, the tray, the Identify plates).
    MonitorChoice => "Monitor", "Monitor";
    MonitorChoiceDesc => "Layar tempat notch muncul", "Screen the notch shows on";
    MonitorIdentify => "Tandai", "Identify";
    MonitorPrimary => "Monitor utama", "Primary monitor";
    MonitorPrimaryTag => "utama", "primary";
    MonitorNotConnected => "tidak terhubung", "not connected";
    MonitorOnlyOne => "Cuma satu monitor terdeteksi", "Only one monitor detected";
    MonitorNumbered => "Monitor", "Monitor";

    // Widgets: Settings -> Widget, the sticky notes on the desktop, the tray
    // entry, the hotkey row and the Notes page's pin.
    SecWidgets => "Widget", "Widgets";
    GroupWidgetsDesktop => "Di desktop", "On the desktop";
    GroupWidgetsGallery => "Galeri", "Gallery";
    WidgetsShow => "Tampilkan widget", "Show widgets";
    WidgetsShowDesc => "Semua widget di desktop", "Every widget on the desktop";
    WidgetsLayer => "Lapisan", "Layer";
    WidgetsLayerDesc => "Catatan yang disematkan selalu di atas",
        "Pinned notes always stay on top";
    WidgetsLayerDesktop => "Di desktop", "On desktop";
    WidgetsLayerTop => "Di atas jendela", "Above windows";
    WidgetsFullscreen => "Saat layar penuh", "In fullscreen";
    WidgetsFullscreenDesc => "Game, video, presentasi", "Games, video, presentations";
    WidgetsHide => "Sembunyikan", "Hide";
    WidgetsKeep => "Biarkan", "Keep";
    WidgetsLock => "Kunci posisi", "Lock position";
    WidgetsLockDesc => "Tidak bisa digeser atau diubah ukurannya", "Cannot be moved or resized";
    WidgetsPeek => "Kucing ngintip", "Peeking cat";
    WidgetsPeekDesc => "Kucing mengintip di catatan yang terakhir disentuh",
        "The cat peeks over the note you touched last";
    StickyName => "Catatan tempel", "Sticky notes";
    StickyDesc => "Catatan berwarna yang nempel di desktop.",
        "Colourful notes that stick to your desktop.";
    StickyAdd => "Tambah catatan tempel", "Add a sticky note";
    StickyMax => "Maksimal 20 catatan tempel", "At most 20 sticky notes";
    StickyFull => "Catatan tempel penuh", "Sticky notes full";
    StickyOnDesktop => "di desktop", "on desktop";
    StickyOf => "dari", "of";
    StickyEdited => "diubah", "edited";
    StickyLineOne => "baris", "line";
    StickyLineMany => "baris", "lines";
    StickyEmpty => "Catatan kosong", "Empty note";
    StickyOnDesktopTip => "Tempel ke desktop", "Stick to desktop";
    StickyOffDesktopTip => "Lepas dari desktop", "Unstick from desktop";
    StickyNew => "Catatan tempel baru", "New sticky note";
    StickyNewMax => "Catatan tempel baru (maksimal 20)", "New sticky note (at most 20)";
    StickyNewDesc => "Catatan kosong baru di desktop", "New empty note on the desktop";
    StickyPinTop => "Sematkan di atas", "Pin on top";
    StickyRoll => "Gulung", "Roll up";
    StickyUnroll => "Buka gulungan", "Unroll";
    StickyTakeOut => "Keluarkan dari tumpukan", "Take out of stack";
    StickyTuck => "Selipkan ke tepi", "Tuck to edge";
    StickyUntuck => "Keluarkan", "Bring out";
    StickyDelete => "Hapus catatan", "Delete note";
    StickyDeleteAgain => "Klik lagi untuk hapus", "Click again to delete";
    StickyTextSize => "Ukuran teks", "Text size";
    StickyChecklist => "Daftar centang", "Checklist";
    // Sticky note format bar, popover, menu, images.
    FmtBold => "Tebal", "Bold";
    FmtItalic => "Miring", "Italic";
    FmtUnderline => "Garis bawah", "Underline";
    FmtStrike => "Coret", "Strikethrough";
    FmtBullet => "Daftar butir", "Bullet list";
    FmtCheck => "Checklist", "Checklist";
    FmtMarker => "Stabilo", "Highlight";
    FmtImage => "Gambar", "Image";
    FmtMore => "Lainnya", "More";
    FmtHlLabel => "Stabilo", "Highlighter";
    FmtFgLabel => "Warna teks", "Text colour";
    FmtHlKuning => "Kuning", "Yellow";
    FmtHlHijau => "Hijau", "Green";
    FmtHlPink => "Pink", "Pink";
    FmtHlBiru => "Biru", "Blue";
    FmtHlNone => "Tanpa stabilo", "No highlight";
    FmtFgAuto => "Otomatis", "Automatic";
    FmtFgMerah => "Merah", "Red";
    FmtFgBiru => "Biru", "Blue";
    FmtFgHijau => "Hijau", "Green";
    FmtPickImages => "Pilih gambar", "Choose images";
    FmtImagesFilter => "Gambar", "Images";
    FmtOpen => "Buka", "Open";
    FmtRemoveImage => "Hapus gambar", "Remove image";
    FmtImgMax => "Maks 4 gambar", "4 images max";
    FmtImgTooBig => "Gambar terlalu besar (maks 25 MB)", "Image too large (25 MB max)";
    FmtImgBad => "Gambar tidak bisa dibaca", "Can't read this image";
    PaperKrem => "Krem", "Cream";
    PaperTerakota => "Terakota", "Terracotta";
    PaperSage => "Sage", "Sage";
    PaperPlum => "Plum", "Plum";
    PaperLaut => "Laut", "Sea";
    PaperGelap => "Gelap", "Dark";
    StickyNoteTitle => "Catatan tempel", "Sticky note";
    StickyRemind => "Ingatkan aku", "Remind me";
    StickyRemind15 => "15 mnt", "15 min";
    StickyRemind1h => "1 jam", "1 hour";
    StickyRemindEvening => "17.00", "17:00";
    StickyRemindTomorrow => "besok 09.00", "tomorrow 09:00";
    StickyRemindCustom => "Atur sendiri\u{2026}", "Custom\u{2026}";
    StickyRemindClear => "Hapus pengingat", "Clear reminder";
    StickySnooze => "Tunda 10 mnt", "Snooze 10 min";
    StickyDayToday => "Hari ini", "Today";
    StickyDayTomorrow => "Besok", "Tomorrow";
    FontVale => "Vale", "Vale";
    FontTangan => "Tangan", "Hand";
    FontMono => "Mono", "Mono";
    StickyFont => "Huruf", "Font";
    StickyPattern => "Kertas", "Paper";
    PatternPolos => "Polos", "Plain";
    PatternGaris => "Garis", "Lines";
    PatternKotak => "Kotak", "Grid";
    PatternTitik => "Titik", "Dots";
    StickyDeco => "Hiasan", "Decoration";
    DecoNone => "Tidak ada", "None";
    DecoSelotip => "Selotip", "Tape";
    DecoPaku => "Paku", "Pin";
    StickyStamp => "Cap", "Stamp";
    StampNone => "Tidak ada", "None";
    StampPenting => "Penting", "Important";
    StampBeres => "Beres", "Done";
    StampMendesak => "Mendesak", "Urgent";
    StampSuka => "Suka", "Love";
    StampNanti => "Nanti", "Later";
    StampIde => "Ide", "Idea";
    StickySendPhone => "Kirim ke HP", "Send to phone";
    StickyList => "Daftar catatan tempel", "Sticky note list";
    StickyListDesc => "Cari dan buka catatan tempel", "Search and open sticky notes";
    StickyListSearch => "Cari catatan\u{2026}", "Search notes\u{2026}";
    StickyListStack => "Tumpukan", "Stack";
    StickyListNone => "Belum ada catatan tempel", "No sticky notes yet";
    StickyListNew => "Buat catatan", "New note";
    StickyListNoMatch => "Tidak ada yang cocok", "No matches";
    StickyListDeleted => "Catatan dihapus", "Note deleted";
    StickyListUndo => "Urungkan", "Undo";
    StickyListOpen => "Buka", "Open";
    StickyListDelete => "Hapus", "Delete";
    StickyOpacity => "Tembus pandang", "See-through";
    WidgetsTilt => "Miring sedikit", "Tilt slightly";
    WidgetsTiltDesc => "Setiap catatan sedikit miring, seperti ditempel tangan.", "Each note leans a little, as if stuck by hand.";
    WidgetsClipFill => "Isi dari clipboard", "Fill from clipboard";
    WidgetsClipFillDesc => "Catatan baru dari hotkey atau tray berisi teks yang disalin", "New notes from the hotkey or tray start with the copied text";
    TplKosong => "Kosong", "Blank";
    TplBelanja => "Belanja", "Shopping";
    TplHariIni => "Hari ini", "Today";
    TplRapat => "Rapat", "Meeting";
    TplIde => "Ide", "Idea";
    TplMingguan => "Mingguan", "This week";
    TplAgenda => "Agenda", "Agenda";
    TplDecisions => "Keputusan", "Decisions";
    TplFollowUp => "Tindak lanjut", "Follow-up";
    TrPrompt => "Ringkas transkrip meeting di bawah ini. Tulis: keputusan yang diambil, action item beserta siapa yang mengerjakan dan tenggatnya kalau disebut, dan pertanyaan yang belum terjawab. \"Saya\" adalah orang yang merekam, \"Peserta\" adalah orang lain di meeting.",
        "Summarize the meeting transcript below. Write: the decisions made, the action items with who does each and its deadline if one is mentioned, and the questions still open. \"Me\" is the person who recorded, \"Others\" are the other people in the meeting.";
    TrFullTranscript => "Transkrip lengkap", "Full transcript";
    TrLangId => "Indonesia", "Indonesian";
    TrLangEn => "Inggris", "English";
    TrLangAuto => "Otomatis", "Auto";
    TrCardTitle => "Transkrip", "Transcripts";

    // Live captions.
    CapTitle => "Caption langsung", "Live captions";
    CapMe => "Saya", "Me";
    CapOthers => "Peserta", "Others";
    CapListening => "Mendengarkan…", "Listening…";
    CapPaused => "Dijeda", "Paused";
    CapBattery => "Pakai baterai, caption dijeda", "On battery, captions paused";
    CapKeepGoing => "Tetap jalan", "Keep going";
    CapNeedsTranskrip => "Caption butuh Transkrip", "Captions need Transcripts";
    CapTurnOnTranskrip => "Nyalakan Transkrip", "Turn on Transcripts";
    CapNeedsModel => "Model transkrip belum ada", "No transcript model yet";
    CapDownload => "Unduh model", "Download model";
    CapDownloading => "Mengunduh model", "Downloading model";
    CapSlow => "Model ini berat buat caption di PC ini. Coba Whisper small.",
        "This model is heavy for captions on this PC. Try Whisper small.";
    CapFailed => "Caption berhenti", "Captions stopped";
    CapCopy => "Salin", "Copy";
    CapSourceLost => "Suara PC terputus", "PC audio lost";
    CapMicLost => "Mic terputus", "Mic lost";
    CapBatteryNotice => "Caption dijeda: pakai baterai", "Captions paused: on battery";
    CapCardTitle => "Caption langsung", "Live captions";
    CapMicRow => "Mic juga", "Mic too";
    CapMicRowDesc => "Omonganmu ikut muncul sebagai \"Saya\"", "Your own speech shows as \"Me\"";
    CapNotchRow => "Caption di notch", "Captions in the notch";
    CapNotchRowDesc => "Kalimat terakhir di notch", "The last sentence in the notch";
    CapBatteryRow => "Tetap jalan pakai baterai", "Keep running on battery";
    CapModelNote => "Pakai model Transkrip yang aktif", "Uses the active Transcripts model";
    TrayCapOn => "Nyalakan caption", "Turn on captions";
    TrayCapOff => "Matikan caption", "Turn off captions";
    HotkeyRowCaptions => "Caption langsung", "Live captions";
    HotkeyRowCaptionsDesc => "Nyala/mati caption langsung", "Live captions on/off";
    RecCaptionSwitch => "Caption langsung", "Live captions";
    TrEnable => "Transkrip", "Transcripts";
    TrEnableDesc => "Ubah rekaman jadi teks di PC ini. Tidak ada audio yang dikirim.",
        "Turns recordings into text on this PC. No audio leaves it.";
    TrOffer => "Unduh {name} ({total} MB, termasuk mesin {engine} MB)?",
        "Download {name} ({total} MB, including the {engine} MB engine)?";
    TrDownload => "Unduh", "Download";
    TrOther => "Pilih model lain", "Choose another model";
    TrCancel => "Batal", "Cancel";
    TrInstalled => "Terpasang", "Installed";
    TrDelete => "Hapus", "Delete";
    TrInUse => "Sedang dipakai", "In use";
    TrSuggested => "disarankan", "suggested";
    TrNoteTurbo => "Paling akurat \u{b7} cepat dengan GPU, lambat tanpa GPU",
        "Most accurate \u{b7} fast with a GPU, slow without one";
    TrNoteSmall => "Ringan \u{b7} lebih cepat tanpa GPU, kadang ada kata yang hilang",
        "Light \u{b7} quicker without a GPU, sometimes drops words";
    TrLanguage => "Bahasa", "Language";
    TrAuto => "Bikin transkrip otomatis setelah rekam", "Transcribe automatically after recording";
    TrAutoDesc => "Mulai sendiri begitu rekaman berhenti", "Starts on its own when a recording stops";
    TrAutoNeedsOn => "Nyalakan Transkrip dulu", "Turn on Transcripts first";
    TrAutoNeedsModel => "Perlu model terpasang", "Needs an installed model";
    TrWhereGpu => "Pakai GPU", "Uses the GPU";
    TrWhereCpu => "Pakai CPU", "Uses the CPU";
    TrUsed => "Terpakai {mb} MB", "{mb} MB used";
    TrBtnCopy => "Salin", "Copy";
    TrBtnOpenMd => "Buka .md", "Open .md";
    TrBtnNote => "Jadikan catatan", "Make a note";
    TrBtnPrompt => "Salin + prompt ringkasan", "Copy + summary prompt";
    TrBtnRetry => "Coba lagi", "Try again";
    TrBtnSettings => "Buka Pengaturan", "Open Settings";
    TrStatusQueued => "Antre, mulai setelah transkrip lain selesai", "Queued, starts after the others finish";
    TrStatusWaiting => "Menunggu model selesai diunduh", "Waiting for the model to download";
    TrStatusHeld => "Dijeda selama merekam", "Paused while recording";
    TrEmpty => "Tidak ada ucapan yang terbaca", "No speech could be made out";
    TrPlayerError => "Audio tidak bisa diputar", "The audio cannot be played";

    // Tooltips: the keys and gestures shown in a tip's key plate.
    TipKeySpace => "Spasi", "Space";
    TipKeyRightClick => "Klik kanan", "Right-click";

    // Tooltips: the sticky note's header, format bar and popovers.
    TipStickyListAbout => "Semua catatan tempel dalam satu jendela, bisa dicari.", "Every sticky note in one window, with search.";
    TipStickyPinOff => "Sematkan di atas", "Keep on top";
    TipStickyPinOffAbout => "Catatan ini tetap di atas jendela lain.", "This note stays above other windows.";
    TipStickyPinOn => "Lepas sematan", "Stop keeping on top";
    TipStickyPinOnAbout => "Jendela lain bisa menutupi catatan ini lagi.", "Other windows can cover this note again.";
    TipStickyNewNote => "Catatan baru", "New note";
    TipStickyNewAbout => "Catatan kosong di samping catatan ini.", "A blank note next to this one.";
    TipStickyCheckAbout => "Ubah baris yang dipilih jadi kotak centang.", "Turn the selected lines into checkboxes.";
    TipStickyStyle => "Warna dan gaya", "Colour and style";
    TipStickyStyleAbout => "Kertas, ukuran, huruf, garis, dan tembus pandang.", "Paper, size, font, lines and see-through.";
    TipStickyMoreAbout => "Pengingat, cap, gulung, selip ke tepi, kirim ke HP, hapus.", "Reminder, stamp, roll up, tuck to edge, send to phone, delete.";
    TipStickyClose => "Lepas dari desktop", "Take off the desktop";
    TipStickyCloseAbout => "Isinya tetap ada di halaman Catatan.", "It stays on the Notes page.";
    TipStickyPrev => "Lembar sebelumnya", "Previous sheet";
    TipStickyNext => "Lembar berikutnya", "Next sheet";
    TipStickyStackAbout => "Roda mouse di strip atas juga bisa.", "The wheel over the top strip works too.";
    TipImageRemoveAbout => "Gambar dikeluarkan dari catatan ini.", "The image is taken out of this note.";
    TipMarkerMenu => "Warna stabilo dan teks", "Highlight and text colour";
    TipMarkerMenuAbout => "Pilih warnanya.", "Pick the colours.";
    TipImageAbout => "Tambah gambar dari file, paling banyak 4.", "Add an image from a file, up to 4.";
    TipBarMoreAbout => "Alat yang tidak muat di bar.", "Tools that do not fit on the bar.";
    TipRemindDayPrev => "Hari sebelumnya", "Previous day";
    TipRemindDayNext => "Hari berikutnya", "Next day";
    TipRemindHourPrev => "Jam sebelumnya", "Earlier hour";
    TipRemindHourNext => "Jam berikutnya", "Later hour";
    TipRemindMinPrev => "Menit sebelumnya", "Earlier minute";
    TipRemindMinNext => "Menit berikutnya", "Later minute";
    TipRemindWheelAbout => "Roda mouse di kolom ini juga bisa.", "The wheel over this column works too.";

    // Tooltips: the sticky note list window.
    TipListNewAbout => "Catatan tempel baru di desktop.", "A new sticky note on the desktop.";
    TipListPinned => "Disematkan di atas", "Kept on top";
    TipListRolled => "Digulung", "Rolled up";
    TipListTucked => "Diselipkan ke tepi", "Tucked to the edge";
    TipListReminder => "Ada pengingat", "Has a reminder";
    TipListReminderPast => "Pengingat sudah berbunyi", "Reminder went off";

    // Tooltips: the closed notch and the open panel's dock and header.
    TipNoticeClose => "Tutup", "Dismiss";
    TipNoticeCloseAbout => "Notifikasi ini hilang tanpa membuka apa pun.", "Closes this notice without opening anything.";
    TipPcAudioMute => "Bisukan suara PC", "Mute PC audio";
    TipPcAudioUnmute => "Nyalakan suara PC", "Unmute PC audio";
    TipSettings => "Pengaturan", "Settings";
    TipSettingsAbout => "Buka jendela Pengaturan Vale.", "Opens Vale's Settings.";
    TipPageCalendar => "Acara Google Calendar per hari, minggu, atau bulan.", "Your Google Calendar by day, week or month.";
    TipPagePomodoro => "Timer fokus dan istirahat.", "Focus and break timer.";
    TipPageRecord => "Rekam audio atau layar; rekaman terakhir dan transkripnya.", "Record audio or the screen; recent recordings and their transcripts.";
    TipPageReminders => "Pengingat dengan jam.", "Reminders with a time.";
    TipPageNotes => "Catatan, bisa ditempel ke desktop.", "Notes you can stick to the desktop.";
    TipPageMedia => "Lagu yang diputar dan liriknya.", "What is playing, with lyrics.";
    TipPageMixer => "Volume utama dan per aplikasi.", "Master and per-app volume.";
    TipPageClipboard => "Riwayat yang kamu salin, bisa dicari dan disematkan.", "What you copied, with search and pins.";
    TipPageNotifications => "Notifikasi Windows yang masuk selama Vale berjalan.", "Windows notifications that came while Vale ran.";
    TipPageShelf => "Tempat singgah file, juga dari dan ke HP.", "A holding place for files, to and from your phone.";
    TipPageSystem => "CPU, RAM, disk, aplikasi tersibuk, dan bersih-bersih.", "CPU, RAM, disks, the busiest apps and clean-up.";
    TipPageDevices => "Flashdisk, disk luar, dan kartu memori, lepas dengan aman.", "Flash drives, external disks and memory cards, eject safely.";
    TipPageCli => "Terminal kecil di panel, dengan folder favorit.", "A small terminal in the panel, with favorite folders.";
    TipPageArrange => "Atur jendela yang terbuka dengan template.", "Lay out your open windows with templates.";
    TipPageShortcuts => "Alat Windows, lompat ke halaman, dan pintasan kamu sendiri.", "Windows tools, page jumps and your own shortcuts.";

    // Tooltips: the controls on the open panel's pages.
    TipCalPrevMonth => "Bulan sebelumnya", "Previous month";
    TipCalNextMonth => "Bulan berikutnya", "Next month";
    TipCalPrevWeek => "Minggu sebelumnya", "Previous week";
    TipCalNextWeek => "Minggu berikutnya", "Next week";
    TipCalPrevDay => "Hari sebelumnya", "Previous day";
    TipCalNextDay => "Hari berikutnya", "Next day";
    TipPlay => "Putar", "Play";
    TipPause => "Jeda", "Pause";
    TipPomoResume => "Lanjutkan", "Resume";
    TipPomoReset => "Ulang", "Reset";
    TipPomoResetAbout => "Kembali ke fokus pertama, timer berhenti.", "Back to the first focus session, timer stopped.";
    TipPomoSkip => "Lewati", "Skip";
    TipPomoSkipAbout => "Fase ini selesai sekarang, fase berikutnya menunggu dimulai.", "This phase ends now, and the next waits for you to start it.";
    TipNoteStickAbout => "Muncul sebagai catatan tempel di desktop.", "Shows as a sticky note on the desktop.";
    TipNoteUnstickAbout => "Catatannya tetap di sini.", "The note stays here.";
    TipMediaPrev => "Lagu sebelumnya", "Previous track";
    TipMediaNext => "Lagu berikutnya", "Next track";
    TipMediaPrevNextAbout => "Geser di notch juga bisa.", "A swipe on the notch works too.";
    TipMediaPlayPauseAbout => "Klik tengah di notch juga bisa.", "A middle click on the notch works too.";
    TipLyricsCard => "Kartu lirik", "Lyrics card";
    TipLyricsCardClose => "Tutup kartu lirik", "Close lyrics card";
    TipLyricsCardAbout => "Lirik di jendela kecil yang melayang.", "Lyrics in a small floating window.";
    TipClipPin => "Sematkan", "Pin";
    TipClipPinAbout => "Tetap ada saat riwayat dibersihkan atau penuh.", "Stays when the history is cleared or full.";
    TipClipUnpin => "Lepas sematan", "Unpin";
    TipClipUnpinAbout => "Ikut terhapus lagi saat riwayat dibersihkan atau penuh.", "Goes again when the history is cleared or full.";
    TipClipPlain => "Salin teks polos", "Copy as plain text";
    TipClipPlainAbout => "Hanya teksnya, tanpa format dan warna.", "Just the text, without formatting or colour.";
    TipClipDeleteAbout => "Dihapus dari riwayat clipboard.", "Removes it from the clipboard history.";
    TipRemove => "Hapus", "Remove";
    TipNotifRemoveAbout => "Hilang dari riwayat Vale. Notifikasi di Windows tidak disentuh.", "Removes it from Vale's history. Windows keeps its own list.";
    TipShelfRemove => "Keluarkan dari rak", "Take off the shelf";
    TipShelfRemoveAbout => "File aslinya tetap ada. Tidak dibagikan ke HP lagi.", "The file itself stays. It is no longer shared with phones.";
    TipBack => "Kembali", "Back";
    TipPhoneShowQr => "Tampilkan kode QR", "Show the QR code";
    TipPhoneShowQrAbout => "Pindai dengan HP untuk tersambung.", "Scan it with a phone to connect.";
    TipArrangePlay => "Terapkan", "Apply";
    TipArrangePlayAbout => "Jendela yang cocok pindah ke tempat yang disimpan.", "Matching windows move to the saved places.";
    TipRecPause => "Jeda rekaman", "Pause recording";
    TipRecResume => "Lanjutkan rekaman", "Resume recording";
    TipRecStartAudioAbout => "Mulai merekam audio sekarang.", "Starts an audio recording now.";
    TipRecStartScreenAbout => "Pilih layar, jendela, atau area, lalu rekaman dimulai.", "Pick a screen, window or area, then the recording starts.";
    TipRecStop => "Hentikan rekaman", "Stop recording";
    TipRecStopAbout => "Rekamannya disimpan.", "The recording is saved.";
    TipRecPickFile => "Transkrip file", "Transcribe a file";
    TipRecPickFileAbout => "Pilih rekaman atau video, lalu transkripnya terbuka.", "Pick a recording or video, then its transcript opens.";
    TipRecFolderAudioAbout => "Folder rekaman audio, di Explorer.", "The audio recordings folder, in Explorer.";
    TipRecFolderScreenAbout => "Folder video rekaman layar, di Explorer.", "The screen recordings folder, in Explorer.";
    TipScreenDiscard => "Buang video", "Discard video";
    TipScreenDiscardAbout => "Rekaman berhenti, videonya ke Recycle Bin. Ditanya dulu.", "Stops and moves the video to the Recycle Bin. Asks first.";
    TipDeviceOpen => "Buka di Explorer", "Open in Explorer";
    TipDeviceDetail => "Detail", "Details";
    TipDeviceDetailAbout => "Model, jalur, dan sistem file.", "Model, connection and file system.";
    TipCliEnlarge => "Perbesar", "Enlarge";
    TipCliShrink => "Perkecil", "Shrink";
    TipCliStar => "Jadikan favorit", "Star";
    TipCliUnstar => "Hapus dari favorit", "Unstar";
    TipCleanBack => "Kembali ke Sistem", "Back to System";

    // Tooltips: the transcript window, the live caption card and the lyrics card.
    TipGrip => "Ubah ukuran", "Resize";
    TipTrPlayAbout => "Dari posisi di bar.", "From the position on the bar.";
    TipTrSeek => "Posisi", "Position";
    TipTrSeekAbout => "Klik atau geser untuk lompat.", "Click or drag to jump.";
    TipTrCopyAbout => "Salin judul dan semua baris transkrip sebagai teks.", "Copies the title and every line of the transcript as text.";
    TipTrPromptAbout => "Salin transkrip dengan perintah ringkasan, siap ditempel ke AI.", "Copies the transcript with a summary prompt, ready to paste into an AI chat.";
    TipTrNoteAbout => "Simpan transkrip ini ke halaman Catatan.", "Saves this transcript to the Notes page.";
    TipTrOpenMdAbout => "Buka file transkrip di aplikasi bawaan Windows.", "Opens the transcript file in the Windows default app.";
    TipTrRedoAbout => "Transkrip ulang, bisa dengan model lain.", "Transcribes again, with another model if you like.";
    TipTrSettingsAbout => "Pengaturan transkrip: nyala, model, bahasa, otomatis.", "Transcript settings: on or off, model, language, auto.";
    TipCapClose => "Tutup caption", "Close captions";
    TipCapCloseAbout => "Caption langsung dimatikan.", "Turns live captions off.";
    TipCapPause => "Jeda caption", "Pause captions";
    TipCapResume => "Lanjutkan caption", "Resume captions";
    TipCapPauseAbout => "Teks yang sudah ada tetap.", "The text so far stays.";
    TipCapCopyAbout => "Salin teks akhir sesi ini, bukan cuma yang terlihat.", "Copies the final text of this session, not just what shows.";
    TipCapLang => "Bahasa caption", "Caption language";
    TipCapLangAbout => "Klik untuk ganti: ID, EN, Auto.", "Click to switch: ID, EN, Auto.";
    TipCapGripAbout => "Ctrl + roda mouse mengubah ukuran teks.", "Ctrl + wheel changes the text size.";

    // Tooltips: the Settings window.
    TipMoveUp => "Naikkan", "Move up";
    TipMoveDown => "Turunkan", "Move down";
    TipDragReorder => "Seret untuk mengubah urutan", "Drag to reorder";
    TipResetDefault => "Kembalikan bawaan", "Reset to default";
    TipResetBecomes => "Jadi", "Becomes";
    TipResetNone => "Tanpa tombol pintas.", "No hotkey.";
    TipOpenBrowser => "Buka di browser", "Open in browser";
}

/// `key` in `lang`.
pub fn text(lang: Lang, key: Key) -> &'static str {
    let (id, en) = pair(key);
    match lang {
        Lang::Id => id,
        Lang::En => en,
    }
}

/// `key` in the current language.
pub fn tr(key: Key) -> &'static str {
    text(lang(), key)
}

/// The notice after a focus phase: the break that follows.
pub fn focus_done_body(lang: Lang, break_min: u32) -> String {
    match lang {
        Lang::Id => format!("Istirahat {break_min} menit · klik untuk mulai"),
        Lang::En => format!("{break_min}-minute break · click to start"),
    }
}

/// The notice after a break: focus session `k` of `total` comes next.
pub fn break_done_body(lang: Lang, k: u32, total: u32) -> String {
    match lang {
        Lang::Id => format!("Fokus sesi {k} dari {total} · klik untuk mulai"),
        Lang::En => format!("Focus session {k} of {total} · click to start"),
    }
}

/// "Sesi 2 dari 4" in the pomodoro header.
pub fn session_of(lang: Lang, k: u32, total: u32) -> String {
    match lang {
        Lang::Id => format!("Sesi {k} dari {total}"),
        Lang::En => format!("Session {k} of {total}"),
    }
}

/// "25 · 5 · 15 menit": focus, break and long break beside the pomodoro title.
pub fn pomo_durations(lang: Lang, focus: u32, short: u32, long: u32) -> String {
    let unit = pick(lang, "menit", "min");
    format!("{focus} · {short} · {long} {unit}")
}

/// "1920×1080 · ± 36 MB/menit": the size a recording will have and about how
/// much a minute of it takes, under "Kualitas video".
pub fn video_estimate_line(lang: Lang, w: u32, h: u32, mib: u32) -> String {
    let unit = pick(lang, "menit", "min");
    format!("{w}×{h} · ± {mib} MB/{unit}")
}

/// "Istirahat 5 menit", "Break · 5 min": the phase after this one.
pub fn phase_minutes(lang: Lang, phase: &str, minutes: u32) -> String {
    match lang {
        Lang::Id => format!("{phase} {minutes} menit"),
        Lang::En => format!("{phase} · {minutes} min"),
    }
}

/// "Bisukan Spotify", "Nyalakan Spotify": the Mixer row's speaker, by what a
/// click does now. A long app name is kept whole (the tip cuts it).
pub fn mute_app(lang: Lang, name: &str, muted: bool) -> String {
    let verb = match (lang, muted) {
        (Lang::Id, false) => "Bisukan",
        (Lang::Id, true) => "Nyalakan",
        (Lang::En, false) => "Mute",
        (Lang::En, true) => "Unmute",
    };
    format!("{verb} {name}").trim_end().to_string()
}

/// "+3 lagi" under a list that does not fit.
pub fn more_count(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("+{n} lagi"),
        Lang::En => format!("+{n} more"),
    }
}

/// The greeting's title for `period`; the morning one is `Key::GoodMorning`.
pub fn greeting_title(lang: Lang, period: DayPeriod) -> &'static str {
    match (period, lang) {
        (DayPeriod::Morning, _) => text(lang, Key::GoodMorning),
        (DayPeriod::Midday, Lang::Id) => "Selamat siang",
        (DayPeriod::Afternoon, Lang::Id) => "Selamat sore",
        (DayPeriod::Midday | DayPeriod::Afternoon, Lang::En) => "Good afternoon",
        (DayPeriod::Evening, Lang::Id) => "Selamat malam",
        (DayPeriod::Evening, Lang::En) => "Good evening",
        (DayPeriod::Night, Lang::Id) => "Masih begadang?",
        (DayPeriod::Night, Lang::En) => "Up late?",
    }
}

/// The short name of `period` in the Settings period line: "Pagi", "Siang",
/// "Sore", "Malam", "Dini hari".
pub fn greeting_period_name(lang: Lang, period: DayPeriod) -> &'static str {
    match (period, lang) {
        (DayPeriod::Morning, Lang::Id) => "Pagi",
        (DayPeriod::Morning, Lang::En) => "Morning",
        (DayPeriod::Midday, Lang::Id) => "Siang",
        (DayPeriod::Midday, Lang::En) => "Noon",
        (DayPeriod::Afternoon, Lang::Id) => "Sore",
        (DayPeriod::Afternoon, Lang::En) => "Afternoon",
        (DayPeriod::Evening, Lang::Id) => "Malam",
        (DayPeriod::Evening, Lang::En) => "Evening",
        (DayPeriod::Night, Lang::Id) => "Dini hari",
        (DayPeriod::Night, Lang::En) => "Late night",
    }
}

/// The five periods as one line for Settings → Maskot → Sapaan: name, first
/// and last hour (two digits) and the greeting's title, joined by " · ".
/// Built from `greeting.rs` so the hours cannot drift from `period_at`.
pub fn greeting_periods_line(lang: Lang) -> String {
    DayPeriod::ALL
        .iter()
        .map(|&p| {
            let (from, to) = p.hours();
            format!(
                "{} {from:02}-{to:02} {}",
                greeting_period_name(lang, p),
                greeting_title(lang, p)
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The greeting's line for `period`, from `counts` (never a "0 acara": a zero
/// part is dropped, and a day with nothing says so):
/// - Morning: "3 acara dan 2 pengingat hari ini" (today's).
/// - Midday and Afternoon: "Masih 2 acara dan 1 pengingat hari ini" (left today).
/// - Evening: "Besok 2 acara" (tomorrow's events; reminders are not counted).
/// - Night: "Sudah larut. Besok 2 acara" (the coming day's events).
pub fn greeting_body(lang: Lang, period: DayPeriod, counts: GreetCounts) -> String {
    let GreetCounts { events, reminders } = counts;
    let (id, en): (&str, &str) = match period {
        DayPeriod::Morning => ("Hari ini belum ada acara", "Nothing planned today"),
        DayPeriod::Midday | DayPeriod::Afternoon => ("Sisa hari ini kosong", "Nothing left today"),
        DayPeriod::Evening => ("Besok belum ada acara", "Nothing planned tomorrow"),
        DayPeriod::Night => (
            "Sudah larut. Besok belum ada acara",
            "It's late. Nothing planned tomorrow",
        ),
    };
    let today = matches!(
        period,
        DayPeriod::Morning | DayPeriod::Midday | DayPeriod::Afternoon
    );
    let reminders = if today { reminders } else { 0 };
    if events == 0 && reminders == 0 {
        return match lang {
            Lang::Id => id,
            Lang::En => en,
        }
        .into();
    }
    match lang {
        Lang::Id => {
            let mut parts = Vec::new();
            if events > 0 {
                parts.push(format!("{events} acara"));
            }
            if reminders > 0 {
                parts.push(format!("{reminders} pengingat"));
            }
            let list = parts.join(" dan ");
            match period {
                DayPeriod::Morning => format!("{list} hari ini"),
                DayPeriod::Midday | DayPeriod::Afternoon => format!("Masih {list} hari ini"),
                DayPeriod::Evening => format!("Besok {list}"),
                DayPeriod::Night => format!("Sudah larut. Besok {list}"),
            }
        }
        Lang::En => {
            let plural = |n: usize, one: &str, many: &str| {
                format!("{n} {}", if n == 1 { one } else { many })
            };
            let mut parts = Vec::new();
            if events > 0 {
                parts.push(plural(events, "event", "events"));
            }
            if reminders > 0 {
                parts.push(plural(reminders, "reminder", "reminders"));
            }
            let list = parts.join(" and ");
            match period {
                DayPeriod::Morning => format!("{list} today"),
                DayPeriod::Midday | DayPeriod::Afternoon => format!("{list} left today"),
                DayPeriod::Evening => format!("{list} tomorrow"),
                DayPeriod::Night => format!("It's late. {list} tomorrow"),
            }
        }
    }
}

pub fn battery_body(lang: Lang, percent: u8) -> String {
    match lang {
        Lang::Id => format!("Sisa {percent}%. Colokin charger."),
        Lang::En => format!("{percent}% left. Plug in the charger."),
    }
}

const MIB: u64 = 1024 * 1024;
const GIB: u64 = MIB * 1024;

/// `value` with one decimal, a comma in Indonesian ("47,2"), a point in
/// English ("47.2").
pub(crate) fn one_decimal(lang: Lang, value: f64) -> String {
    let s = format!("{value:.1}");
    match lang {
        Lang::Id => s.replace('.', ","),
        Lang::En => s,
    }
}

/// A size as the System page shows it: "512 MB" under 1 GB, "47,2 GB"
/// ("47.2 GB"), "1,8 TB" from 1000 GB. Binary units, as Explorer counts.
pub fn format_size(lang: Lang, bytes: u64) -> String {
    let mb = (bytes as f64 / MIB as f64).round();
    // Just under 1 GB rounds up to 1024 MB: "1,0 GB" instead.
    if mb < 1024.0 {
        return format!("{mb:.0} MB");
    }
    let gb = bytes as f64 / GIB as f64;
    // Compared as shown (one decimal), so 999.96 GB is not "1000,0 GB".
    if (gb * 10.0).round() >= 10_000.0 {
        return format!("{} TB", one_decimal(lang, gb / 1024.0));
    }
    format!("{} GB", one_decimal(lang, gb))
}

/// "Masih dipakai: Excel", "Still in use: Excel": the app that kept a
/// device from being ejected.
pub fn veto_in_use_by(lang: Lang, app: &str) -> String {
    match lang {
        Lang::Id => format!("Masih dipakai: {app}"),
        Lang::En => format!("Still in use: {app}"),
    }
}

/// The eject refusal that names a Windows service.
pub fn veto_service(lang: Lang, name: &str) -> String {
    match lang {
        Lang::Id => format!("Masih dipakai layanan Windows: {name}"),
        Lang::En => format!("In use by a Windows service: {name}"),
    }
}

/// The plugged-in notice's title: "SanDisk Ultra tersambung".
pub fn device_plugged(lang: Lang, name: &str) -> String {
    match lang {
        Lang::Id => format!("{name} tersambung"),
        Lang::En => format!("{name} connected"),
    }
}

/// The plugged-in notice's body: "E: · 38,9 GB kosong", "E: · 38.9 GB free".
pub fn device_free(lang: Lang, letter: char, free: &str) -> String {
    match lang {
        Lang::Id => format!("{letter}: · {free} kosong"),
        Lang::En => format!("{letter}: · {free} free"),
    }
}

/// "3 perangkat", "3 devices" (the safe-to-remove notice's body when the
/// names would not fit).
pub fn device_count(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("{n} perangkat"),
        Lang::En => format!("{n} {}", if n == 1 { "device" } else { "devices" }),
    }
}

/// "Vale 3.11.0 tersedia", "Vale 3.11.0 is available": the update notice's
/// title and the Tentang row's description.
pub fn update_available(lang: Lang, version: &str) -> String {
    match lang {
        Lang::Id => format!("Vale {version} tersedia"),
        Lang::En => format!("Vale {version} is available"),
    }
}

/// The tray item while an update is known: "Update ke 3.11.0", "Update to 3.11.0".
pub fn update_menu_install(lang: Lang, version: &str) -> String {
    match lang {
        Lang::Id => format!("Update ke {version}"),
        Lang::En => format!("Update to {version}"),
    }
}

/// "Mengunduh pembaruan… 42%", "Downloading update… 42%".
pub fn update_downloading(lang: Lang, pct: u8) -> String {
    match lang {
        Lang::Id => format!("Mengunduh pembaruan\u{2026} {pct}%"),
        Lang::En => format!("Downloading update\u{2026} {pct}%"),
    }
}

/// "Update gagal: tidak ada koneksi", "Update failed: no connection".
pub fn update_failed(lang: Lang, reason: &str) -> String {
    match lang {
        Lang::Id => format!("Update gagal: {reason}"),
        Lang::En => format!("Update failed: {reason}"),
    }
}

/// An import refused because the file comes from a newer Vale.
pub fn settings_file_newer(lang: Lang, version: &str) -> String {
    match lang {
        Lang::Id => format!("File ini dari Vale yang lebih baru (versi {version}). Perbarui Vale dulu."),
        Lang::En => format!("This file comes from a newer Vale ({version}). Update Vale first."),
    }
}

/// A manual check with nothing newer: "Vale sudah versi terbaru (3.10.0)".
pub fn update_up_to_date(lang: Lang, version: &str) -> String {
    match lang {
        Lang::Id => format!("Vale sudah versi terbaru ({version})"),
        Lang::En => format!("Vale is up to date ({version})"),
    }
}

/// After an update restarted Vale: "Diperbarui ke 3.11.0", "Updated to 3.11.0".
pub fn update_done(lang: Lang, version: &str) -> String {
    match lang {
        Lang::Id => format!("Diperbarui ke {version}"),
        Lang::En => format!("Updated to {version}"),
    }
}

/// The Tentang row's description when idle: "Versi terpasang 3.10.0".
pub fn update_installed(lang: Lang, version: &str) -> String {
    match lang {
        Lang::Id => format!("Versi terpasang {version}"),
        Lang::En => format!("Installed version {version}"),
    }
}

/// The Susun pick step's title: "Ubin 2 dari 3", "Tile 2 of 3".
pub fn arrange_tile_of(lang: Lang, n: usize, total: usize) -> String {
    match lang {
        Lang::Id => format!("Ubin {n} dari {total}"),
        Lang::En => format!("Tile {n} of {total}"),
    }
}

/// The notice after an arrangement: "Tersusun \u{b7} 3 jendela", with
/// " \u{b7} 1 dilewati" when some tiles got no window.
pub fn arrange_done(lang: Lang, moved: usize, skipped: Option<usize>) -> String {
    let mut out = match lang {
        Lang::Id => format!("Tersusun \u{b7} {moved} jendela"),
        Lang::En => format!(
            "Arranged \u{b7} {moved} window{}",
            if moved == 1 { "" } else { "s" }
        ),
    };
    if let Some(k) = skipped {
        out.push_str(&match lang {
            Lang::Id => format!(" \u{b7} {k} dilewati"),
            Lang::En => format!(" \u{b7} {k} skipped"),
        });
    }
    out
}

/// The notice after a saved arrangement ran: "2 dari 3 jendela, 1 dilewati",
/// "2 of 3 windows, 1 skipped".
pub fn arrange_saved_run(lang: Lang, moved: usize, wanted: usize, skipped: usize) -> String {
    match lang {
        Lang::Id => format!("{moved} dari {wanted} jendela, {skipped} dilewati"),
        Lang::En => format!("{moved} of {wanted} windows, {skipped} skipped"),
    }
}

/// The primary entry's muted note: "sekarang: 2", "now: 2".
pub fn monitor_now(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("sekarang: {n}"),
        Lang::En => format!("now: {n}"),
    }
}

/// What a hotkey row's reset button gives back: "Jadi Ctrl+Alt+L.", or that
/// the row has no hotkey by default.
pub fn reset_to(lang: Lang, combo: Option<String>) -> String {
    match combo {
        Some(c) => format!("{} {c}.", text(lang, Key::TipResetBecomes)),
        None => text(lang, Key::TipResetNone).to_owned(),
    }
}

/// A monitor entry: "2 \u{b7} LG 27UL".
pub fn monitor_entry(n: usize, name: &str) -> String {
    format!("{n} \u{b7} {name}")
}

/// An entry's muted size note: "2560\u{d7}1440", with ", utama" / ", primary" on the primary.
pub fn monitor_size(lang: Lang, w: i32, h: i32, primary: bool) -> String {
    let tag = text(lang, Key::MonitorPrimaryTag);
    if primary {
        format!("{w}\u{d7}{h}, {tag}")
    } else {
        format!("{w}\u{d7}{h}")
    }
}

/// A monitor chip: "1 \u{b7} 2560\u{d7}1440".
pub fn arrange_monitor_chip(n: usize, w: i32, h: i32) -> String {
    format!("{n} \u{b7} {w}\u{d7}{h}")
}

/// A built-in layout's name.
pub fn arrange_builtin_name(lang: Lang, b: crate::arrange::BuiltIn) -> &'static str {
    use crate::arrange::BuiltIn;
    let key = match b {
        BuiltIn::Cols2 => Key::ArrangeCols2,
        BuiltIn::Cols3 => Key::ArrangeCols3,
        BuiltIn::Main2 => Key::ArrangeMain2,
        BuiltIn::Grid4 => Key::ArrangeGrid4,
        BuiltIn::TwoThirds => Key::ArrangeTwoThirds,
        BuiltIn::Main3 => Key::ArrangeMain3,
    };
    text(lang, key)
}

/// The Devices page's chip: "3 tersambung", "3 connected".
pub fn devices_connected(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("{n} tersambung"),
        Lang::En => format!("{n} connected"),
    }
}

/// A day heading in the Rekam list: "Hari ini", "Kemarin", else "8 Nov".
pub fn rec_day_label(lang: Lang, day: NaiveDate, today: NaiveDate) -> String {
    match near(day, today) {
        Some(n @ (Near::Today | Near::Yesterday)) => near_word(lang, n, true).to_string(),
        _ => short_date(lang, day),
    }
}

/// The label of a Rekam row's transcript pill: "Transkrip", "Antre",
/// "Menunggu model", "42%", "Lihat" or "Gagal". The off state shows the
/// "Transkrip" label too (the pill is drawn dimmed).
pub fn tr_pill(lang: Lang, mark: crate::model::RowTranscript) -> String {
    use crate::model::RowTranscript as M;
    match mark {
        M::Off | M::None => text(lang, Key::TrPillTranscribe).to_owned(),
        M::Queued => text(lang, Key::TrPillQueued).to_owned(),
        M::Waiting => text(lang, Key::TrPillWaiting).to_owned(),
        M::Running(n) => format!("{n}%"),
        M::Done => text(lang, Key::TrPillDone).to_owned(),
        M::Failed => text(lang, Key::TrPillFailed).to_owned(),
    }
}

/// "Server menolak unduhan (404)": the server's answer to a model download.
pub fn tr_why_refused(lang: Lang, code: u16) -> String {
    text(lang, Key::TrWhyRefused).replace("{code}", &code.to_string())
}

/// The Transkrip card's first-run offer: "Unduh Whisper small (191 MB,
/// termasuk mesin 19 MB)?".
pub fn tr_offer(lang: Lang, name: &str, total_mb: u64, engine_mb: u64) -> String {
    text(lang, Key::TrOffer)
        .replace("{name}", name)
        .replace("{total}", &total_mb.to_string())
        .replace("{engine}", &engine_mb.to_string())
}

/// "Terpakai 610 MB": the space the engine and the models take.
pub fn tr_used(lang: Lang, mb: u64) -> String {
    text(lang, Key::TrUsed).replace("{mb}", &mb.to_string())
}

/// "220 / 555 MB": a model download's progress (the same in both languages).
pub fn tr_progress(done_mb: u64, total_mb: u64) -> String {
    format!("{done_mb} / {total_mb} MB")
}

/// A running transcript's progress in its window: "42% · sekitar 6 mnt lagi"
/// ("42% · about 6 min left"), or "42%" before there is an estimate.
pub fn tr_running(lang: Lang, pct: u8, eta: Option<std::time::Duration>) -> String {
    let Some(eta) = eta else {
        return format!("{pct}%");
    };
    let left = rec_duration(lang, eta);
    match lang {
        Lang::Id => format!("{pct}% \u{b7} sekitar {left} lagi"),
        Lang::En => format!("{pct}% \u{b7} about {left} left"),
    }
}

/// The zoom level the REC notch shows: whole levels bare ("2×"), halves with
/// the language's decimal separator ("2,5×" in Indonesian, "2.5×" in English).
/// `×` is U+00D7.
pub fn zoom_label(lang: Lang, level: f64) -> String {
    let halves = (level * 2.0).round() as i64;
    let whole = halves / 2;
    if halves % 2 == 0 {
        return format!("{whole}\u{D7}");
    }
    let point = match lang {
        Lang::Id => ',',
        Lang::En => '.',
    };
    format!("{whole}{point}5\u{D7}")
}

/// A recording's length on the Rekam page: "45 dtk", "42 mnt", "1 j 05 mnt"
/// ("45 s", "42 min", "1 h 05 min"). Minutes round down.
pub fn rec_duration(lang: Lang, d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let (h, m) = (secs / 3600, secs / 60 % 60);
    let (hour, min, sec) = match lang {
        Lang::Id => ("j", "mnt", "dtk"),
        Lang::En => ("h", "min", "s"),
    };
    if h > 0 {
        format!("{h} {hour} {m:02} {min}")
    } else if m > 0 {
        format!("{m} {min}")
    } else {
        format!("{secs} {sec}")
    }
}

/// The count of recordings left in the folder beyond the list: "9 lagi di
/// folder" ("9 more in the folder").
pub fn rec_more(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("{n} lagi di folder"),
        Lang::En => format!("{n} more in the folder"),
    }
}

/// The mute badge on the Rekam page: "Dibisukan · Ctrl+Alt+M", or the bare
/// word when the mute has no hotkey.
pub fn rec_muted(lang: Lang, hotkey: Option<&str>) -> String {
    let word = text(lang, Key::RecMuted);
    match hotkey {
        Some(keys) => format!("{word} \u{b7} {keys}"),
        None => word.to_string(),
    }
}

/// The hint under an empty Rekam list: "Tekan Rekam, atau Ctrl+Alt+R dari
/// mana aja" ("Press Record, or Ctrl+Alt+R from anywhere"), or "Tekan Rekam"
/// ("Press Record") with no hotkey.
pub fn rec_empty_hint(lang: Lang, hotkey: Option<&str>) -> String {
    match (lang, hotkey) {
        (Lang::Id, Some(keys)) => format!("Tekan Rekam, atau {keys} dari mana aja"),
        (Lang::Id, None) => "Tekan Rekam".to_string(),
        (Lang::En, Some(keys)) => format!("Press Record, or {keys} from anywhere"),
        (Lang::En, None) => "Press Record".to_string(),
    }
}

/// A recording's quality on the Rekam page: "1080p · 30 fps", or the
/// Settings word for the screen's own size: "Asli · 30 fps" ("Original · 30 fps").
pub fn rec_quality_label(lang: Lang, q: VideoQuality, fps: u32) -> String {
    let size = match q.max_short() {
        Some(px) => format!("{px}p"),
        None => text(lang, Key::ScreenSettingsQualityOriginal).to_string(),
    };
    format!("{size} \u{b7} {fps} fps")
}

/// How long the QR code lives: "berlaku 4:52", "valid 4:52".
pub fn phone_valid_for(lang: Lang, secs: u64) -> String {
    let clock = format!("{}:{:02}", secs / 60, secs % 60);
    match lang {
        Lang::Id => format!("berlaku {clock}"),
        Lang::En => format!("valid {clock}"),
    }
}

/// The network line of the QR view: "Rumah-5G \u{b7} pribadi", or
/// "Kenzie_5G \u{b7} publik" on a Public network.
pub fn phone_network(lang: Lang, name: &str, trust: crate::phone::net::Trust) -> String {
    use crate::phone::net::Trust;
    let word = match trust {
        Trust::Private => Key::PhoneNetPrivate,
        Trust::Public => Key::PhoneNetPublic,
    };
    format!("{name} \u{b7} {}", text(lang, word))
}

/// Step 2's sub line once a phone joined: "Tersambung: Android",
/// "Connected: 2 phones".
pub fn phone_connected(lang: Lang, kinds: &[crate::phone::DeviceKind]) -> String {
    format!(
        "{}: {}",
        text(lang, Key::PhoneConnected),
        phone_devices_chip(lang, kinds)
    )
}

/// What a phone is called: "Android", "iPhone", "iPad", else "HP".
pub fn phone_kind(lang: Lang, kind: crate::phone::DeviceKind) -> &'static str {
    use crate::phone::DeviceKind;
    text(
        lang,
        match kind {
            DeviceKind::Android => Key::PhoneKindAndroid,
            DeviceKind::IPhone => Key::PhoneKindIPhone,
            DeviceKind::IPad => Key::PhoneKindIPad,
            DeviceKind::Other => Key::PhoneKindOther,
        },
    )
}

/// The header chip of the receiving view: one phone by its kind
/// ("Android"), several by their number ("2 HP", "2 phones").
pub fn phone_devices_chip(lang: Lang, kinds: &[crate::phone::DeviceKind]) -> String {
    match kinds {
        [] => String::new(),
        [one] => phone_kind(lang, *one).to_owned(),
        many => match lang {
            Lang::Id => format!("{} HP", many.len()),
            Lang::En => format!("{} phones", many.len()),
        },
    }
}

/// A received file's size: "812 KB", "5 MB", "1,8 GB".
pub fn phone_size(lang: Lang, bytes: u64) -> String {
    if bytes < MIB {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format_size(lang, bytes)
    }
}

/// The receiving view's footer: "3 berkas · 55 MB → Unduhan\Vale".
pub fn phone_received_summary(lang: Lang, files: usize, bytes: u64) -> String {
    let size = phone_size(lang, bytes);
    match lang {
        Lang::Id => format!("{files} berkas \u{b7} {size} \u{2192} Unduhan\\Vale"),
        Lang::En => {
            let noun = if files == 1 { "file" } else { "files" };
            format!("{files} {noun} \u{b7} {size} \u{2192} Downloads\\Vale")
        }
    }
}

/// A drive's meter: "sisa 47,2 GB dari 338,4 GB", "47.2 GB free of 338.4 GB".
pub fn disk_free(lang: Lang, free: u64, total: u64) -> String {
    let (f, t) = (format_size(lang, free), format_size(lang, total));
    match lang {
        Lang::Id => format!("sisa {f} dari {t}"),
        Lang::En => format!("{f} free of {t}"),
    }
}

/// The RAM meter: "14,2 / 31,8 GB · 44%", both in GB.
pub fn ram_usage(lang: Lang, used: u64, total: u64, percent: u8) -> String {
    let gb = |b: u64| one_decimal(lang, b as f64 / GIB as f64);
    format!("{} / {} GB · {percent}%", gb(used), gb(total))
}

/// "Uptime 6 j 1 m" ("6 h 1 m"); from a day "Uptime 2 hr 6 j" ("2 d 6 h");
/// under an hour only the minutes.
pub fn uptime(lang: Lang, up: std::time::Duration) -> String {
    let (day, hour) = pick(lang, ("hr", "j"), ("d", "h"));
    let mins = up.as_secs() / 60;
    let (d, h, m) = (mins / 1440, mins / 60 % 24, mins % 60);
    let span = if d > 0 {
        format!("{d} {day} {h} {hour}")
    } else if h > 0 {
        format!("{h} {hour} {m} m")
    } else {
        format!("{m} m")
    };
    format!("Uptime {span}")
}

/// Under the CPU ring: "CPU · 8 inti", "CPU · 8 cores" (drawn in capitals).
pub fn cpu_cores(lang: Lang, cores: u32) -> String {
    format!("CPU · {cores} {}", pick(lang, "inti", "cores"))
}

/// A drive's meter name and the start of its notice: "Disk C:".
pub fn disk_name(letter: char) -> String {
    format!("Disk {letter}:")
}

/// The low-disk notice: "Disk C: tinggal 9,6 GB", "Disk C: 9.6 GB left".
pub fn disk_low_title(lang: Lang, letter: char, free: u64) -> String {
    let (name, f) = (disk_name(letter), format_size(lang, free));
    match lang {
        Lang::Id => format!("{name} tinggal {f}"),
        Lang::En => format!("{name} {f} left"),
    }
}

/// A cleaner size: "38 MB" under 0,1 GB, else "1,8 GB" ("1.8 GB") with
/// one decimal like the design, "1,2 TB" from 1000 GB.
pub fn clean_size(lang: Lang, bytes: u64) -> String {
    let mb = (bytes as f64 / MIB as f64).round();
    if mb < 102.0 {
        return format!("{mb:.0} MB");
    }
    let gb = bytes as f64 / GIB as f64;
    if (gb * 10.0).round() >= 10_000.0 {
        return format!("{} TB", one_decimal(lang, gb / 1024.0));
    }
    format!("{} GB", one_decimal(lang, gb))
}

/// A cleaner row's name and its subtitle.
pub fn clean_cat(cat: CleanCat) -> (Key, Key) {
    match cat {
        CleanCat::TempLogs => (Key::CleanTempLogs, Key::CleanTempLogsSub),
        CleanCat::BrowserCache => (Key::CleanBrowser, Key::CleanBrowserSub),
        CleanCat::AppCache => (Key::CleanApps, Key::CleanAppsSub),
        CleanCat::DevCache => (Key::CleanDev, Key::CleanDevSub),
        CleanCat::AutodeskAdobe => (Key::CleanAdobe, Key::CleanAdobeSub),
        CleanCat::OldInstallers => (Key::CleanInstallers, Key::CleanInstallersSub),
        CleanCat::WindowsUpdate => (Key::CleanUpdate, Key::CleanUpdateSub),
    }
}

/// The cleaner's footer: "Dipilih 2,3 GB", "Selected 2.3 GB".
pub fn clean_picked(lang: Lang, bytes: u64) -> String {
    let size = clean_size(lang, bytes);
    match lang {
        Lang::Id => format!("Dipilih {size}"),
        Lang::En => format!("Selected {size}"),
    }
}

/// The cleaner's button: "Bersihkan 2,3 GB", "Clean 2.3 GB".
pub fn clean_go(lang: Lang, bytes: u64) -> String {
    let size = clean_size(lang, bytes);
    match lang {
        Lang::Id => format!("Bersihkan {size}"),
        Lang::En => format!("Clean {size}"),
    }
}

/// The armed button: "Yakin? Hapus 2,3 GB", "Sure? Delete 2.3 GB".
pub fn clean_confirm(lang: Lang, bytes: u64) -> String {
    let size = clean_size(lang, bytes);
    match lang {
        Lang::Id => format!("Yakin? Hapus {size}"),
        Lang::En => format!("Sure? Delete {size}"),
    }
}

/// The browser row while some were open at the scan: "Chrome, Edge
/// terbuka · dilewati", "Chrome, Edge open · skipped".
pub fn clean_browsers_open(lang: Lang, names: &[&str]) -> String {
    let names = names.join(", ");
    match lang {
        Lang::Id => format!("{names} terbuka · dilewati"),
        Lang::En => format!("{names} open · skipped"),
    }
}

/// The done notice's title: "2,3 GB dibebaskan", "2.3 GB freed".
pub fn clean_freed(lang: Lang, bytes: u64) -> String {
    let size = clean_size(lang, bytes);
    match lang {
        Lang::Id => format!("{size} dibebaskan"),
        Lang::En => format!("{size} freed"),
    }
}

/// What the done notice adds under its title, joined by " · ".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CleanNote<'a> {
    /// The drive measured and its free space now.
    pub drive: Option<(char, u64)>,
    /// Files in use (or denied) left in place.
    pub skipped: u32,
    /// Browsers left alone because they were open.
    pub browsers: &'a [&'a str],
    /// Windows Update was ticked but UAC was refused or the helper failed.
    pub admin_skipped: bool,
}

/// The first of `ways` (longest first) that `fits`, else the shortest.
pub fn fit_one(ways: &[String], fits: &dyn Fn(&str) -> bool) -> String {
    ways.iter()
        .find(|w| fits(w))
        .or(ways.last())
        .cloned()
        .unwrap_or_default()
}

/// Joins `parts` (in their order of importance, each a list of ways to say
/// it, longest first) with " · " into the fullest line that `fits`: every
/// part in its longest wording, then every part shorter, and only when even
/// the shortest wording does not fit, the least important part left out
/// (and so on). One part that never fits is still shown in its shortest
/// wording, never dropped.
fn fit_parts(parts: &[Vec<String>], fits: &dyn Fn(&str) -> bool) -> String {
    let levels = parts.iter().map(Vec::len).max().unwrap_or(0);
    let line = |keep: usize, level: usize| {
        parts[..keep]
            .iter()
            .filter_map(|ways| ways.get(level.min(ways.len().saturating_sub(1))))
            .cloned()
            .collect::<Vec<String>>()
            .join(" · ")
    };
    for keep in (1..=parts.len()).rev() {
        for level in 0..levels {
            let l = line(keep, level);
            if fits(&l) {
                return l;
            }
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        line(1, levels.saturating_sub(1))
    }
}

/// "Disk C: sekarang sisa 49,5 GB · 3 item dilewati (sedang dipakai)", or
/// as short as "C: sisa 49,5 GB · 3 item dilewati", then the open browsers
/// and a skipped Windows Update, as far as `fits` allows (the notice is
/// 360 px wide; its shape stays).
pub fn clean_done_body(lang: Lang, note: &CleanNote, fits: &dyn Fn(&str) -> bool) -> String {
    let mut parts = Vec::new();
    if let Some((letter, free)) = note.drive {
        let (name, f) = (disk_name(letter), format_size(lang, free));
        parts.push(match lang {
            Lang::Id => vec![
                format!("{name} sekarang sisa {f}"),
                format!("{letter}: sisa {f}"),
            ],
            Lang::En => vec![
                format!("{name} now {f} free"),
                format!("{letter}: {f} free"),
            ],
        });
    }
    if note.skipped > 0 {
        let n = note.skipped;
        parts.push(match lang {
            Lang::Id => vec![
                format!("{n} item dilewati (sedang dipakai)"),
                format!("{n} item dilewati"),
            ],
            Lang::En if n == 1 => vec!["1 item skipped (in use)".into(), "1 item skipped".into()],
            Lang::En => vec![
                format!("{n} items skipped (in use)"),
                format!("{n} items skipped"),
            ],
        });
    }
    if !note.browsers.is_empty() {
        let names = note.browsers.join(", ");
        let n = note.browsers.len();
        let mut ways = vec![match lang {
            Lang::Id => format!("{names} terbuka"),
            Lang::En => format!("{names} open"),
        }];
        if n > 1 {
            ways.push(match lang {
                Lang::Id => format!("{n} browser terbuka"),
                Lang::En => format!("{n} browsers open"),
            });
        }
        parts.push(ways);
    }
    if note.admin_skipped {
        parts.push(vec![
            match lang {
                Lang::Id => "Windows Update dilewati",
                Lang::En => "Windows Update skipped",
            }
            .to_string(),
        ]);
    }
    fit_parts(&parts, fits)
}

/// The Mole notice's title: "Mole selesai · 2,4 GB dibebaskan", shorter
/// ("Mole · 2,4 GB dibebaskan", "2,4 GB dibebaskan") when `fits` says the
/// full one would not fit beside the "Log" chip.
pub fn mole_done_title(lang: Lang, freed: u64, fits: &dyn Fn(&str) -> bool) -> String {
    let size = clean_size(lang, freed);
    let ways = match lang {
        Lang::Id => [
            format!("Mole selesai · {size} dibebaskan"),
            format!("Mole · {size} dibebaskan"),
            format!("{size} dibebaskan"),
        ],
        Lang::En => [
            format!("Mole done · {size} freed"),
            format!("Mole · {size} freed"),
            format!("{size} freed"),
        ],
    };
    fit_one(&ways, fits)
}

/// "Diukur dari sisa disk C:", and that the system part was skipped (UAC
/// refused) when it fits.
pub fn mole_done_body(
    lang: Lang,
    letter: char,
    admin_skipped: bool,
    fits: &dyn Fn(&str) -> bool,
) -> String {
    let mut parts = vec![match lang {
        Lang::Id => vec![
            format!("Diukur dari sisa disk {letter}:"),
            format!("Diukur di {letter}:"),
        ],
        Lang::En => vec![
            format!("Measured from free space on {letter}:"),
            format!("Measured on {letter}:"),
        ],
    }];
    if admin_skipped {
        parts.push(match lang {
            Lang::Id => vec!["Sistem (admin) dilewati".into(), "Sistem dilewati".into()],
            Lang::En => vec!["System (admin) skipped".into(), "System skipped".into()],
        });
    }
    fit_parts(&parts, fits)
}

/// "Mole gagal · kode 1"; "Mole berhenti" without a code (killed).
pub fn mole_failed_title(lang: Lang, code: Option<i32>) -> String {
    match (lang, code) {
        (Lang::Id, Some(c)) => format!("Mole gagal · kode {c}"),
        (Lang::En, Some(c)) => format!("Mole failed · code {c}"),
        (Lang::Id, None) => "Mole berhenti".into(),
        (Lang::En, None) => "Mole stopped".into(),
    }
}

/// What a failed clean still freed, measured: "0,4 GB dibebaskan sebelum
/// berhenti", or "Lihat log di Bersihkan".
pub fn mole_failed_body(lang: Lang, freed: u64) -> String {
    let mb = (freed as f64 / MIB as f64).round();
    if mb < 1.0 {
        return match lang {
            Lang::Id => "Lihat log di Bersihkan".into(),
            Lang::En => "See the log in Clean up".into(),
        };
    }
    let size = clean_size(lang, freed);
    match lang {
        Lang::Id => format!("{size} dibebaskan sebelum berhenti"),
        Lang::En => format!("{size} freed before it stopped"),
    }
}

/// Under the Mole mode's title: who picks, and that it is permanent; a
/// warning when the version is not the tested 1.30.x.
pub fn mole_intro(lang: Lang, version: Option<&str>, tested: bool) -> String {
    match (lang, version, tested) {
        (Lang::Id, Some(v), true) => {
            format!("Mole {v} memilih sendiri apa yang dibersihkan · hapus permanen")
        }
        (Lang::En, Some(v), true) => format!("Mole {v} picks what it cleans · deleted for good"),
        (Lang::Id, Some(v), false) => {
            format!("Mole {v} belum diuji (dibuat untuk 1.30.x) · hapus permanen")
        }
        (Lang::En, Some(v), false) => {
            format!("Mole {v} is untested (made for 1.30.x) · deleted for good")
        }
        (Lang::Id, None, _) => {
            "Versi Mole tak dikenal (dibuat untuk 1.30.x) · hapus permanen".into()
        }
        (Lang::En, None, _) => "Unknown Mole version (made for 1.30.x) · deleted for good".into(),
    }
}

/// The Mole footer: "Pratinjau: ± 2,4 GB", or "Pratinjau: lihat log" when
/// nothing could be read.
pub fn mole_preview(lang: Lang, total: Option<u64>) -> String {
    let what = match total {
        Some(b) => format!("± {}", clean_size(lang, b)),
        None => text(lang, Key::MoleSeeLog).to_owned(),
    };
    match lang {
        Lang::Id => format!("Pratinjau: {what}"),
        Lang::En => format!("Preview: {what}"),
    }
}

/// Beside the armed button: "± 2,4 GB · 29 item".
pub fn mole_total_items(lang: Lang, bytes: u64, entries: usize) -> String {
    let size = clean_size(lang, bytes);
    match lang {
        Lang::Id => format!("± {size} · {entries} item"),
        Lang::En if entries == 1 => format!("± {size} · 1 item"),
        Lang::En => format!("± {size} · {entries} items"),
    }
}

/// After a clean: "2,4 GB dibebaskan · C: 47,2 GB → 49,6 GB".
pub fn mole_result(lang: Lang, freed: u64, drive: Option<(char, u64, u64)>) -> String {
    let freed = clean_freed(lang, freed);
    match drive {
        Some((letter, before, after)) => format!(
            "{freed} · {letter}: {} \u{2192} {}",
            format_size(lang, before),
            format_size(lang, after)
        ),
        None => freed,
    }
}

/// The RAM notice: "RAM terpakai 93%", "RAM 93% used".
pub fn ram_high_title(lang: Lang, percent: u8) -> String {
    match lang {
        Lang::Id => format!("RAM terpakai {percent}%"),
        Lang::En => format!("RAM {percent}% used"),
    }
}

/// An invalid config.json was moved aside to `name`.
pub fn config_copied_to(lang: Lang, name: &str) -> String {
    match lang {
        Lang::Id => format!("Disalin ke {name} — cek lagi isinya"),
        Lang::En => format!("Copied to {name} — check its contents"),
    }
}

/// "Versi 2.1.0" on the settings window's About page.
pub fn version_label(lang: Lang, version: &str) -> String {
    match lang {
        Lang::Id => format!("Versi {version}"),
        Lang::En => format!("Version {version}"),
    }
}

/// The CLI's `cd` to a folder that does not exist.
pub fn cd_not_found(lang: Lang, path: &str) -> String {
    match lang {
        Lang::Id => format!("cd: folder tidak ada: {path}"),
        Lang::En => format!("cd: no such folder: {path}"),
    }
}

/// The CLI could not start the shell.
pub fn cli_start_failed(lang: Lang, error: &str) -> String {
    match lang {
        Lang::Id => format!("Gagal menjalankan: {error}"),
        Lang::En => format!("Couldn't run it: {error}"),
    }
}

/// The CLI's quiet line after it followed Explorer: "↳ pindah ke D:\x
/// (folder Explorer terakhir)".
pub fn cli_moved_to(lang: Lang, path: &str) -> String {
    match lang {
        Lang::Id => format!("\u{21B3} pindah ke {path} (folder Explorer terakhir)"),
        Lang::En => format!("\u{21B3} moved to {path} (last Explorer folder)"),
    }
}

/// The CLI's note after leaving a followed Explorer folder for the user's
/// own last one.
pub fn cli_back_to(lang: Lang, path: &str) -> String {
    match lang {
        Lang::Id => format!("\u{21B3} kembali ke {path} (folder terakhirmu)"),
        Lang::En => format!("\u{21B3} back to {path} (your last folder)"),
    }
}

/// The CLI's note when a star finds the favourites full.
pub fn cli_favorites_full(lang: Lang) -> String {
    let cap = crate::cli_folders::FAVORITES_CAP;
    match lang {
        Lang::Id => format!("\u{21B3} Favorit penuh ({cap}): lepas bintang yang lain dulu"),
        Lang::En => format!("\u{21B3} Favourites are full ({cap}): unstar another first"),
    }
}

/// One of the CLI's own notes (`cli_moved_to`, `cli_back_to`), in either
/// language: drawn quiet. Program output that merely starts with the same
/// arrow is not one.
pub fn is_cli_note(line: &str) -> bool {
    let notes = [
        ("\u{21B3} pindah ke ", " (folder Explorer terakhir)"),
        ("\u{21B3} moved to ", " (last Explorer folder)"),
        ("\u{21B3} kembali ke ", " (folder terakhirmu)"),
        ("\u{21B3} back to ", " (your last folder)"),
        ("\u{21B3} Favorit penuh (", "lepas bintang yang lain dulu"),
        ("\u{21B3} Favourites are full (", "unstar another first"),
    ];
    notes
        .iter()
        .any(|(start, end)| line.starts_with(start) && line.ends_with(end))
}

/// When a recent folder was used, at the right of its menu row: "baru
/// saja", "2 mnt lalu", "3 jam lalu", "kemarin", "20 Okt" (and in English).
pub fn folder_ago(lang: Lang, ago: crate::cli_folders::Ago) -> String {
    use crate::cli_folders::Ago;
    match ago {
        Ago::JustNow => text(lang, Key::CliJustNow).to_owned(),
        Ago::Minutes(n) => match lang {
            Lang::Id => format!("{n} mnt lalu"),
            Lang::En => format!("{n} min ago"),
        },
        Ago::Hours(n) => match lang {
            Lang::Id => format!("{n} jam lalu"),
            Lang::En => format!("{n} h ago"),
        },
        Ago::Yesterday => near_word(lang, Near::Yesterday, false).to_owned(),
        Ago::On(d) => short_date(lang, d),
    }
}

/// A Clipboard kind's label ("Teks", "Tautan", …).
pub fn clip_kind_key(kind: crate::clipboard::ClipKind) -> Key {
    use crate::clipboard::ClipKind;
    match kind {
        ClipKind::Text => Key::ClipKindText,
        ClipKind::Link => Key::ClipKindLink,
        ClipKind::Code => Key::ClipKindCode,
        ClipKind::Color => Key::ClipKindColor,
        ClipKind::Image => Key::ClipKindImage,
        ClipKind::Files => Key::ClipKindFiles,
    }
}

/// The Clipboard search field's placeholder: "Cari di 48 item…".
pub fn clip_search_in(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("Cari di {n} item…"),
        Lang::En if n == 1 => "Search 1 item…".to_owned(),
        Lang::En => format!("Search {n} items…"),
    }
}

/// A search root's name in a result row ("Unduhan").
pub fn search_root(kind: crate::search_files::RootKind) -> &'static str {
    use crate::search_files::RootKind;
    tr(match kind {
        RootKind::Desktop => Key::SearchRootDesktop,
        RootKind::Documents => Key::SearchRootDocuments,
        RootKind::Downloads => Key::SearchRootDownloads,
        RootKind::Pictures => Key::SearchRootPictures,
        RootKind::Videos => Key::SearchRootVideos,
        RootKind::Music => Key::SearchRootMusic,
    })
}

/// A file's size as a search row says it: "1 KB" at the least, "245 KB",
/// then "2,4 MB" up to 10 MB, then `format_size` ("245 MB", "1,2 GB").
pub fn search_file_size(lang: Lang, bytes: u64) -> String {
    const KIB: u64 = 1024;
    let kb = ((bytes + KIB / 2) / KIB).max(1);
    if kb < 1024 {
        format!("{kb} KB")
    } else if bytes < 10 * MIB {
        format!("{} MB", one_decimal(lang, bytes as f64 / MIB as f64))
    } else {
        format_size(lang, bytes)
    }
}

/// A folder's item count in a search row: "1 item", "12 item" ("12
/// items"), "999+ item" for a folder with more than 999.
pub fn search_folder_items(lang: Lang, n: usize) -> String {
    let shown = if n > 999 {
        "999+".to_owned()
    } else {
        n.to_string()
    };
    match lang {
        Lang::En if n != 1 => format!("{shown} items"),
        Lang::Id | Lang::En => format!("{shown} item"),
    }
}

/// The quick search's web row: `Cari "kopi susu" di web`.
pub fn search_web_title(lang: Lang, query: &str) -> String {
    match lang {
        Lang::Id => format!("Cari \"{query}\" di web"),
        Lang::En => format!("Search the web for \"{query}\""),
    }
}

/// An image card's meta line: "Screenshot · 1280 × 720", "Gambar · 640 × 480".
pub fn clip_image_meta(lang: Lang, screenshot: bool, w: u32, h: u32) -> String {
    let what = if screenshot {
        "Screenshot"
    } else {
        text(lang, Key::ClipKindImage)
    };
    format!("{what} · {w} × {h}")
}

/// A copied image whose original is no longer kept: "Screenshot · pratinjau
/// kecil, aslinya tidak tersimpan".
pub fn clip_image_small(lang: Lang, screenshot: bool) -> String {
    let what = if screenshot {
        "Screenshot"
    } else {
        text(lang, Key::ClipKindImage)
    };
    format!("{what} · {}", text(lang, Key::ClipImageSmall))
}

/// Copied files in the "Disalin" notice: "laporan.pdf", "3 file".
pub fn clip_files(lang: Lang, n: usize, first: &str) -> String {
    match (lang, n) {
        (_, 1) => first.to_owned(),
        (Lang::Id, n) => format!("{n} file"),
        (Lang::En, n) => format!("{n} files"),
    }
}

/// The copy preview's meta line for a copied text that has no second line:
/// "Teks · 42 karakter", "Code · 1 character". A link or a colour is its
/// kind's name alone (the number tells nothing there).
pub fn clip_text_meta(lang: Lang, kind: crate::clipboard::ClipKind, chars: usize) -> String {
    use crate::clipboard::ClipKind;
    let name = text(lang, clip_kind_key(kind));
    if matches!(kind, ClipKind::Link | ClipKind::Color) {
        return name.to_owned();
    }
    match (lang, chars) {
        (Lang::Id, n) => format!("{name} · {n} karakter"),
        (Lang::En, 1) => format!("{name} · 1 character"),
        (Lang::En, n) => format!("{name} · {n} characters"),
    }
}

/// The copy preview's body for an image: "1280 × 720".
/// (Tested by `copy_preview_texts`.)
pub fn clip_preview_size(w: u32, h: u32) -> String {
    format!("{w} × {h}")
}

/// The screenshot preview's size line: "1280 × 720 · 245 KB"; from
/// 1000 KB on "1,8 MB" ("1.8 MB"). Binary units, as Explorer counts.
pub fn shot_size(lang: Lang, w: u32, h: u32, bytes: u64) -> String {
    let kb = (bytes as f64 / 1024.0).round();
    let size = if kb < 1000.0 {
        format!("{} KB", kb.max(1.0))
    } else {
        format!("{} MB", one_decimal(lang, bytes as f64 / MIB as f64))
    };
    format!("{w} × {h} · {size}")
}

/// "Dihapus" and what it did: the file (to the Recycle Bin, none matched,
/// or still there) and the clipboard (emptied or left as it was). The
/// design's "Dihapus · clipboard dikosongkan" when both went.
pub fn shot_deleted(
    lang: Lang,
    file: crate::screenshot::FileOutcome,
    emptied: bool,
) -> (&'static str, String) {
    use crate::screenshot::FileOutcome;
    let title = match file {
        FileOutcome::Failed => text(lang, Key::ShotDeleteFailed),
        _ => text(lang, Key::ShotDeleted),
    };
    let clip = text(
        lang,
        if emptied {
            Key::ShotClipEmptied
        } else {
            Key::ShotClipUnchanged
        },
    );
    let body = match (file, emptied) {
        (FileOutcome::Recycled, true) => clip.to_owned(),
        (FileOutcome::Recycled, false) => format!("{} · {clip}", text(lang, Key::ShotRecycled)),
        (FileOutcome::NoFile, _) => format!("{} · {clip}", text(lang, Key::ShotNoFile)),
        (FileOutcome::Failed, _) => format!("{} · {clip}", text(lang, Key::ShotFileStays)),
    };
    (title, body)
}

/// "8 dtk", "8 s": the "Lama tampil" stepper's value.
pub fn seconds_short(lang: Lang, secs: u32) -> String {
    format!("{secs} {}", text(lang, Key::SecondsShort))
}

/// "Sudah dipakai Buka Clipboard": the message under a hotkey row whose new
/// combo another row already has; `label` is that row's name.
pub fn hotkey_used_by(lang: Lang, label: &str) -> String {
    match lang {
        Lang::Id => format!("Sudah dipakai {label}"),
        Lang::En => format!("Already used by {label}"),
    }
}

/// The sticky note list's count: "7 catatan" / "7 notes", "1 note".
pub fn count_label(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("{n} catatan"),
        Lang::En if n == 1 => "1 note".to_owned(),
        Lang::En => format!("{n} notes"),
    }
}

/// "Tumpukan 2/3" / "Stack 2/3": a stack sheet's badge in the sticky note
/// list (`pos` is 1-based).
pub fn stack_badge(lang: Lang, pos: usize, len: usize) -> String {
    format!("{} {pos}/{len}", text(lang, Key::StickyListStack))
}

/// "Tumpukan, 3 lembar" / "Stack, 3 sheets": the stack mark's tip in the
/// sticky note list.
pub fn tip_list_stack(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("Tumpukan, {n} lembar"),
        Lang::En => format!("Stack, {n} {}", if n == 1 { "sheet" } else { "sheets" }),
    }
}

/// "1 dari 4 dicentang" / "1 of 4 checked": the checklist mark's tip.
pub fn tip_list_checked(lang: Lang, done: usize, total: usize) -> String {
    match lang {
        Lang::Id => format!("{done} dari {total} dicentang"),
        Lang::En => format!("{done} of {total} checked"),
    }
}

/// "2 gambar" / "2 images": the images mark's tip.
pub fn tip_list_images(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Id => format!("{n} gambar"),
        Lang::En => format!("{n} {}", if n == 1 { "image" } else { "images" }),
    }
}

/// A running command's time on the clock: "0:42", "12:05", "1:02:03".
pub fn run_clock(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// How long a finished command took: "2,1 dtk" ("2.1 s") under a minute,
/// then "1 mnt 12 dtk" ("1 min 12 s"), from an hour "1 jam 5 mnt"
/// ("1 h 5 min").
pub fn run_duration(lang: Lang, took: std::time::Duration) -> String {
    let (sec, min, hour) = match lang {
        Lang::Id => ("dtk", "mnt", "jam"),
        Lang::En => ("s", "min", "h"),
    };
    // Tenths, rounded; 59.96 s is a whole minute.
    let tenths = (took.as_millis() + 50) / 100;
    if tenths < 600 {
        let (whole, tenth) = (tenths / 10, tenths % 10);
        let point = pick(lang, ",", ".");
        return format!("{whole}{point}{tenth} {sec}");
    }
    let secs = (took.as_millis() + 500) / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h} {hour} {m} {min}")
    } else {
        format!("{m} {min} {s} {sec}")
    }
}

/// The CLI header chip while a command runs: "Berjalan · 0:42".
pub fn cli_running(lang: Lang, elapsed: std::time::Duration) -> String {
    let word = pick(lang, "Berjalan", "Running");
    format!("{word} · {}", run_clock(elapsed))
}

/// The chip once a command ended: "selesai · 2,1 dtk" for exit 0,
/// "exit 1 · 3,8 dtk" otherwise, "dihentikan · 3,8 dtk" after Stop. A run
/// that took no measurable time (a builtin) leaves the time out.
pub fn cli_result(
    lang: Lang,
    code: Option<i32>,
    killed: bool,
    took: std::time::Duration,
) -> String {
    let head = match (killed, code) {
        (true, _) => text(lang, Key::CliStopped).to_owned(),
        (false, Some(0)) => pick(lang, "selesai", "done").to_owned(),
        (false, Some(c)) => format!("exit {c}"),
        (false, None) => "exit ?".to_owned(),
    };
    if took.is_zero() {
        head
    } else {
        format!("{head} · {}", run_duration(lang, took))
    }
}

/// "Gagal simpan: {reason}".
pub fn save_failed_because(lang: Lang, reason: &str) -> String {
    format!("{}: {reason}", text(lang, Key::SaveFailed))
}

/// "Google menolak ({message})".
pub fn google_refused(lang: Lang, message: &str) -> String {
    format!("{} ({message})", text(lang, Key::GoogleRefused))
}

/// A 2xx answer from Google that is not what was asked for.
pub fn unreadable_reply(lang: Lang, status: u16) -> String {
    match lang {
        Lang::Id => format!("HTTP {status}: jawaban tidak terbaca"),
        Lang::En => format!("HTTP {status}: unreadable response"),
    }
}

/// Claude Code's settings file at `path` does not parse.
pub fn not_valid_json(lang: Lang, path: &str, error: &str) -> String {
    match lang {
        Lang::Id => format!("{path} bukan JSON yang valid: {error}"),
        Lang::En => format!("{path} is not valid JSON: {error}"),
    }
}

/// Claude Code's settings file at `path` has hooks in a shape the island
/// cannot add to.
pub fn unknown_format(lang: Lang, path: &str) -> String {
    match lang {
        Lang::Id => format!("format {path} tidak dikenali"),
        Lang::En => format!("unrecognized format in {path}"),
    }
}

/// The page the browser shows after the Google login.
pub fn oauth_page(lang: Lang, key: Key) -> String {
    format!(
        "<!doctype html><html lang=\"{}\"><meta charset=\"utf-8\"><title>Vale</title>\
         <p style=\"font:16px sans-serif\">{}</p>",
        lang.code(),
        text(lang, key)
    )
}

const MONTHS_ID: [&str; 12] = [
    "Januari",
    "Februari",
    "Maret",
    "April",
    "Mei",
    "Juni",
    "Juli",
    "Agustus",
    "September",
    "Oktober",
    "November",
    "Desember",
];
const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_SHORT_ID: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des",
];
const MONTHS_SHORT_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// Monday first.
const DAYS_ID: [&str; 7] = [
    "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu", "Minggu",
];
const DAYS_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const DAYS_SHORT_ID: [&str; 7] = ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"];
const DAYS_SHORT_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn pick<T>(lang: Lang, id: T, en: T) -> T {
    match lang {
        Lang::Id => id,
        Lang::En => en,
    }
}

fn weekday(d: NaiveDate) -> usize {
    d.weekday().num_days_from_monday() as usize
}

/// "Sen", "Mon".
pub fn day_short(lang: Lang, d: NaiveDate) -> &'static str {
    weekday_short(lang, d.weekday())
}

/// "Sen", "Mon".
pub fn weekday_short(lang: Lang, d: Weekday) -> &'static str {
    pick(lang, DAYS_SHORT_ID, DAYS_SHORT_EN)[d.num_days_from_monday() as usize]
}

/// "Senin", "Monday".
pub fn weekday_long(lang: Lang, d: Weekday) -> &'static str {
    pick(lang, DAYS_ID, DAYS_EN)[d.num_days_from_monday() as usize]
}

/// A weekly reminder's day chip: "S S R K J S M" / "M T W T F S S".
pub fn weekday_letter(lang: Lang, d: Weekday) -> &'static str {
    let key = match d {
        Weekday::Mon => Key::DayLetterMon,
        Weekday::Tue => Key::DayLetterTue,
        Weekday::Wed => Key::DayLetterWed,
        Weekday::Thu => Key::DayLetterThu,
        Weekday::Fri => Key::DayLetterFri,
        Weekday::Sat => Key::DayLetterSat,
        Weekday::Sun => Key::DayLetterSun,
    };
    text(lang, key)
}

/// "25 Okt", "25 Oct".
pub fn short_date(lang: Lang, d: NaiveDate) -> String {
    let months = pick(lang, MONTHS_SHORT_ID, MONTHS_SHORT_EN);
    format!("{} {}", d.day(), months[d.month0() as usize])
}

/// "Oktober 2026", "October 2026".
pub fn month_title(lang: Lang, d: NaiveDate) -> String {
    let months = pick(lang, MONTHS_ID, MONTHS_EN);
    format!("{} {}", months[d.month0() as usize], d.year())
}

/// "Senin, 26 Oktober", "Monday, 26 October": the day view's title and the
/// month view's caption.
pub fn day_title_long(lang: Lang, d: NaiveDate) -> String {
    let days = pick(lang, DAYS_ID, DAYS_EN);
    let months = pick(lang, MONTHS_ID, MONTHS_EN);
    format!(
        "{}, {} {}",
        days[weekday(d)],
        d.day(),
        months[d.month0() as usize]
    )
}

/// "Rabu, 4 Nov", "Wednesday, 4 Nov": a day's name and its short date.
pub fn day_title_short(lang: Lang, d: NaiveDate) -> String {
    format!("{}, {}", day_long(lang, d), short_date(lang, d))
}

/// The week from `monday`: "19 – 25 Okt 2026", "28 Sep – 4 Okt 2026",
/// "29 Des 2025 – 4 Jan 2026".
pub fn week_title(lang: Lang, monday: NaiveDate) -> String {
    let sunday = monday + chrono::Days::new(6);
    let end = format!("{} {}", short_date(lang, sunday), sunday.year());
    if monday.year() != sunday.year() {
        format!("{} {} – {end}", short_date(lang, monday), monday.year())
    } else if monday.month() != sunday.month() {
        format!("{} – {end}", short_date(lang, monday))
    } else {
        format!("{} – {end}", monday.day())
    }
}

/// "09.12" in Indonesian, "09:12" in English.
pub fn time_hm(lang: Lang, t: NaiveDateTime) -> String {
    t.format(pick(lang, "%H.%M", "%H:%M")).to_string()
}

/// A time of day as `time_hm` writes it: "09.30", "09:30".
pub fn time_label(lang: Lang, t: NaiveTime) -> String {
    time_hm(lang, NaiveDate::MIN.and_time(t))
}

/// "Senin", "Monday".
pub fn day_long(lang: Lang, d: NaiveDate) -> &'static str {
    pick(lang, DAYS_ID, DAYS_EN)[weekday(d)]
}

/// The editors' date chip: "Sen, 26 Okt 2026", "Mon, 26 Oct 2026". Built
/// from the app's own names, never from the Windows locale.
pub fn editor_date(lang: Lang, d: NaiveDate) -> String {
    format!(
        "{}, {} {}",
        day_short(lang, d),
        short_date(lang, d),
        d.year()
    )
}

/// The calendar popover's footer: "Rab, 28 Okt", "Wed, 28 Oct".
pub fn day_date_short(lang: Lang, d: NaiveDate) -> String {
    format!("{}, {}", day_short(lang, d), short_date(lang, d))
}

/// A day next to today, named rather than dated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Near {
    Yesterday,
    Today,
    Tomorrow,
}

fn near(d: NaiveDate, today: NaiveDate) -> Option<Near> {
    match d.signed_duration_since(today).num_days() {
        -1 => Some(Near::Yesterday),
        0 => Some(Near::Today),
        1 => Some(Near::Tomorrow),
        _ => None,
    }
}

/// "hari ini", "besok", "kemarin" / "today", "tomorrow", "yesterday", with
/// a capital first letter when it starts the label.
fn near_word(lang: Lang, n: Near, capital: bool) -> &'static str {
    match (n, capital) {
        (Near::Today, false) => pick(lang, "hari ini", "today"),
        (Near::Today, true) => pick(lang, "Hari ini", "Today"),
        (Near::Tomorrow, false) => pick(lang, "besok", "tomorrow"),
        (Near::Tomorrow, true) => pick(lang, "Besok", "Tomorrow"),
        (Near::Yesterday, false) => pick(lang, "kemarin", "yesterday"),
        (Near::Yesterday, true) => pick(lang, "Kemarin", "Yesterday"),
    }
}

/// `s` with `d`'s year added when it is not `today`'s.
fn with_year(s: String, d: NaiveDate, today: NaiveDate) -> String {
    if d.year() == today.year() {
        s
    } else {
        format!("{s} {}", d.year())
    }
}

/// "hari ini", "besok", "kemarin", else "Rab, 28 Okt" (with the year when
/// it is not this year's).
fn day_phrase(lang: Lang, d: NaiveDate, today: NaiveDate) -> String {
    match near(d, today) {
        Some(n) => near_word(lang, n, false).to_owned(),
        None => with_year(day_date_short(lang, d), d, today),
    }
}

/// The reminder editor's date chip. A one-off names its day: "Hari ini,
/// 26 Okt", "Besok, 27 Okt", "Kemarin, 25 Okt", "Rab, 28 Okt"; a repeating
/// one says when it starts: "Mulai hari ini", "Mulai besok", "Mulai Sen,
/// 26 Okt". The year is added when it is not today's.
pub fn reminder_date_chip(lang: Lang, repeat: Repeat, date: NaiveDate, today: NaiveDate) -> String {
    match repeat {
        Repeat::Once => {
            let label = match near(date, today) {
                Some(n) => format!("{}, {}", near_word(lang, n, true), short_date(lang, date)),
                None => day_date_short(lang, date),
            };
            with_year(label, date, today)
        }
        Repeat::Daily | Repeat::Weekly => {
            let from = pick(lang, "Mulai", "From");
            let when = match near(date, today) {
                Some(n @ (Near::Today | Near::Tomorrow)) => near_word(lang, n, false).to_owned(),
                _ => with_year(day_date_short(lang, date), date, today),
            };
            format!("{from} {when}")
        }
    }
}

/// The reminder editor's one-line summary: `lead`, then `bold` (drawn in the
/// text colour, medium), then `tail`. `passed` marks a one-off already due,
/// drawn in the error colour.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReminderSummary {
    pub lead: String,
    pub bold: String,
    pub tail: String,
    pub passed: bool,
}

impl ReminderSummary {
    /// The whole line as plain text.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn plain(&self) -> String {
        format!("{}{}{}", self.lead, self.bold, self.tail)
    }
}

/// The date a repeating reminder starting on `start` rings next: the first
/// of its days from `start` on whose `time` is after `now`, by the store's
/// own rule (`reminders::next_repeat`): every day for `Daily`, the weekdays
/// in `days` (or `start`'s when empty) for `Weekly`. A one-off rings on
/// `start`.
pub fn next_occurrence(
    repeat: Repeat,
    start: NaiveDate,
    days: &[Weekday],
    time: NaiveTime,
    now: NaiveDateTime,
) -> NaiveDate {
    let ring = |d: NaiveDate| (d.and_time(time) > now).then_some(());
    reminders::next_repeat(repeat, start, days, true, now.date(), ring).map_or(start, |(d, ())| d)
}

/// "4 jam lagi", "25 menit lagi" / "in 4 hours", "in 1 hour", "in 25 min":
/// how soon a one-off due within a day rings. `None` from a day out.
fn due_in(lang: Lang, seconds: i64) -> Option<String> {
    if seconds <= 0 || seconds >= 24 * 3600 {
        return None;
    }
    let minutes = (seconds / 60).max(1);
    Some(if minutes < 60 {
        match lang {
            Lang::Id => format!("{minutes} menit lagi"),
            Lang::En => format!("in {minutes} min"),
        }
    } else {
        let hours = minutes / 60;
        match lang {
            Lang::Id => format!("{hours} jam lagi"),
            Lang::En if hours == 1 => "in 1 hour".to_owned(),
            Lang::En => format!("in {hours} hours"),
        }
    })
}

/// The summary under the reminder's repeat row, as of `now`:
/// "Sekali, **hari ini jam 16.30** · 4 jam lagi",
/// "Sekali, **kemarin jam 16.30** · sudah lewat",
/// "Tiap hari jam **08.00** · berikutnya besok",
/// "Tiap **Rabu jam 09.00** · berikutnya Rab, 28 Okt",
/// "Tiap **Sen, Rab, Jum jam 09.00** · berikutnya Rab, 28 Okt" (and in
/// English). `days` are a weekly reminder's weekdays (`Reminder::days`;
/// empty: `date`'s).
pub fn reminder_summary(
    lang: Lang,
    repeat: Repeat,
    date: NaiveDate,
    days: &[Weekday],
    time: NaiveTime,
    now: NaiveDateTime,
) -> ReminderSummary {
    let today = now.date();
    let at = time_label(lang, time);
    let days = reminders::sorted_days(days);
    let next = || {
        let d = next_occurrence(repeat, date, &days, time, now);
        let word = pick(lang, "berikutnya", "next");
        format!(" · {word} {}", day_phrase(lang, d, today))
    };
    match repeat {
        Repeat::Once => {
            let day = day_phrase(lang, date, today);
            let due = date.and_time(time);
            let passed = due <= now;
            let tail = if passed {
                format!(" · {}", pick(lang, "sudah lewat", "already passed"))
            } else {
                due_in(lang, due.signed_duration_since(now).num_seconds())
                    .map(|s| format!(" · {s}"))
                    .unwrap_or_default()
            };
            ReminderSummary {
                lead: pick(lang, "Sekali, ", "Once, ").to_owned(),
                bold: match lang {
                    Lang::Id => format!("{day} jam {at}"),
                    Lang::En => format!("{day} at {at}"),
                },
                tail,
                passed,
            }
        }
        Repeat::Daily => ReminderSummary {
            lead: pick(lang, "Tiap hari jam ", "Every day at ").to_owned(),
            bold: at,
            tail: next(),
            passed: false,
        },
        Repeat::Weekly => {
            // One day by its full name, several by their short ones.
            let weekdays = match days[..] {
                [] => day_long(lang, date).to_owned(),
                [one] => weekday_long(lang, one).to_owned(),
                _ => days
                    .iter()
                    .map(|d| weekday_short(lang, *d))
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            ReminderSummary {
                lead: pick(lang, "Tiap ", "Every ").to_owned(),
                bold: match lang {
                    Lang::Id => format!("{weekdays} jam {at}"),
                    Lang::En => format!("{weekdays} at {at}"),
                },
                tail: next(),
                passed: false,
            }
        }
    }
}

/// A length of time: "30 mnt", "1 j", "1 j 30 mnt" / "30 min", "1 hr",
/// "1 hr 30 min". Negative lengths read as zero.
pub fn duration_label(lang: Lang, minutes: i64) -> String {
    let minutes = minutes.max(0);
    let (h, m) = (minutes / 60, minutes % 60);
    let (hour, min) = pick(lang, ("j", "mnt"), ("hr", "min"));
    match (h, m) {
        (0, m) => format!("{m} {min}"),
        (h, 0) => format!("{h} {hour}"),
        (h, m) => format!("{h} {hour} {m} {min}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sticky_list_labels_count_and_badge() {
        assert_eq!(count_label(Lang::Id, 7), "7 catatan");
        assert_eq!(count_label(Lang::Id, 1), "1 catatan");
        assert_eq!(count_label(Lang::En, 7), "7 notes");
        assert_eq!(count_label(Lang::En, 1), "1 note");
        assert_eq!(count_label(Lang::En, 0), "0 notes");
        assert_eq!(stack_badge(Lang::Id, 2, 3), "Tumpukan 2/3");
        assert_eq!(stack_badge(Lang::En, 2, 3), "Stack 2/3");
        assert_eq!(text(Lang::Id, Key::StickyList), "Daftar catatan tempel");
        assert_eq!(text(Lang::En, Key::StickyList), "Sticky note list");
        assert_eq!(text(Lang::Id, Key::StickyListSearch), "Cari catatan\u{2026}");
        assert_eq!(text(Lang::Id, Key::StickyListNone), "Belum ada catatan tempel");
        assert_eq!(text(Lang::En, Key::StickyListNoMatch), "No matches");
    }

    /// True when `s` holds a run like "10/26/2026": digits, slash, digits,
    /// slash, digits (the Windows short date the old picker showed).
    fn has_slashed_date(s: &str) -> bool {
        let b = s.as_bytes();
        let digits = |mut i: usize| {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            (i > start).then_some(i)
        };
        (0..b.len()).any(|i| {
            let slash = |j: Option<usize>| j.filter(|&j| b.get(j) == Some(&b'/')).map(|j| j + 1);
            slash(digits(i))
                .and_then(|j| slash(digits(j)))
                .and_then(digits)
                .is_some()
        })
    }

    #[test]
    fn slashed_date_detector_works() {
        assert!(has_slashed_date("Today: 10/26/2026"));
        assert!(has_slashed_date("1/2/3"));
        assert!(!has_slashed_date("Sen, 26 Okt 2026"));
        assert!(!has_slashed_date("10/26"));
    }

    #[test]
    fn editor_date_names_weekday_day_month_and_year() {
        let d = date(2026, 10, 26);
        assert_eq!(editor_date(Lang::Id, d), "Sen, 26 Okt 2026");
        assert_eq!(editor_date(Lang::En, d), "Mon, 26 Oct 2026");
        assert_eq!(
            editor_date(Lang::Id, date(2026, 10, 28)),
            "Rab, 28 Okt 2026"
        );
        assert_eq!(editor_date(Lang::En, date(2027, 1, 3)), "Sun, 3 Jan 2027");
        assert_eq!(day_date_short(Lang::Id, date(2026, 10, 28)), "Rab, 28 Okt");
        assert_eq!(day_date_short(Lang::En, date(2026, 10, 28)), "Wed, 28 Oct");
        assert_eq!(day_long(Lang::Id, date(2026, 10, 30)), "Jumat");
        assert_eq!(day_long(Lang::En, date(2026, 10, 30)), "Friday");
        // A whole year of dates, both languages: never the Windows short date.
        let mut d = date(2026, 1, 1);
        while d.year() == 2026 {
            for lang in [Lang::Id, Lang::En] {
                for s in [editor_date(lang, d), day_date_short(lang, d)] {
                    assert!(!has_slashed_date(&s), "{s}");
                    assert!(s.contains(&d.day().to_string()), "{s}");
                }
                assert!(editor_date(lang, d).ends_with(" 2026"));
            }
            d = d.succ_opt().unwrap();
        }
    }

    #[test]
    fn durations_and_times_in_both_languages() {
        assert_eq!(duration_label(Lang::Id, 15), "15 mnt");
        assert_eq!(duration_label(Lang::Id, 30), "30 mnt");
        assert_eq!(duration_label(Lang::Id, 60), "1 j");
        assert_eq!(duration_label(Lang::Id, 90), "1 j 30 mnt");
        assert_eq!(duration_label(Lang::Id, 150), "2 j 30 mnt");
        assert_eq!(duration_label(Lang::En, 30), "30 min");
        assert_eq!(duration_label(Lang::En, 60), "1 hr");
        assert_eq!(duration_label(Lang::En, 90), "1 hr 30 min");
        assert_eq!(duration_label(Lang::En, -5), "0 min");
        let t = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
        assert_eq!(time_label(Lang::Id, t), "09.30");
        assert_eq!(time_label(Lang::En, t), "09:30");
        let late = NaiveTime::from_hms_opt(21, 15, 0).unwrap();
        assert_eq!(time_label(Lang::Id, late), "21.15");
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(d: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
        d.and_hms_opt(h, m, 0).unwrap()
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    /// Monday 26 October 2026, 12:30.
    fn monday_noon() -> NaiveDateTime {
        at(date(2026, 10, 26), 12, 30)
    }

    fn summary(lang: Lang, repeat: Repeat, d: NaiveDate, t: NaiveTime) -> ReminderSummary {
        reminder_summary(lang, repeat, d, &[], t, monday_noon())
    }

    #[test]
    fn reminder_date_chip_names_the_day_or_the_start() {
        let today = date(2026, 10, 26);
        let chip = |lang, repeat, d| reminder_date_chip(lang, repeat, d, today);
        let once = Repeat::Once;
        assert_eq!(chip(Lang::Id, once, today), "Hari ini, 26 Okt");
        assert_eq!(chip(Lang::Id, once, date(2026, 10, 27)), "Besok, 27 Okt");
        assert_eq!(chip(Lang::Id, once, date(2026, 10, 25)), "Kemarin, 25 Okt");
        assert_eq!(chip(Lang::Id, once, date(2026, 10, 28)), "Rab, 28 Okt");
        assert_eq!(chip(Lang::Id, once, date(2027, 1, 5)), "Sel, 5 Jan 2027");
        assert_eq!(chip(Lang::En, once, today), "Today, 26 Oct");
        assert_eq!(chip(Lang::En, once, date(2026, 10, 27)), "Tomorrow, 27 Oct");
        assert_eq!(
            chip(Lang::En, once, date(2026, 10, 25)),
            "Yesterday, 25 Oct"
        );
        assert_eq!(chip(Lang::En, once, date(2026, 10, 28)), "Wed, 28 Oct");
        // Tomorrow in another year keeps its year.
        assert_eq!(
            reminder_date_chip(Lang::Id, once, date(2027, 1, 1), date(2026, 12, 31)),
            "Besok, 1 Jan 2027"
        );
        for repeat in [Repeat::Daily, Repeat::Weekly] {
            assert_eq!(chip(Lang::Id, repeat, today), "Mulai hari ini");
            assert_eq!(chip(Lang::Id, repeat, date(2026, 10, 27)), "Mulai besok");
            assert_eq!(
                chip(Lang::Id, repeat, date(2026, 10, 25)),
                "Mulai Min, 25 Okt"
            );
            assert_eq!(chip(Lang::En, repeat, today), "From today");
            assert_eq!(chip(Lang::En, repeat, date(2026, 10, 27)), "From tomorrow");
            assert_eq!(chip(Lang::En, repeat, date(2026, 11, 2)), "From Mon, 2 Nov");
        }
        assert_eq!(
            reminder_date_chip(
                Lang::Id,
                Repeat::Daily,
                date(2026, 10, 26),
                date(2026, 10, 27)
            ),
            "Mulai Sen, 26 Okt"
        );
    }

    #[test]
    fn one_off_summaries_name_the_day_and_how_soon() {
        let mon = date(2026, 10, 26);
        let s = summary(Lang::Id, Repeat::Once, mon, hm(16, 30));
        assert_eq!(s.plain(), "Sekali, hari ini jam 16.30 · 4 jam lagi");
        assert_eq!(s.bold, "hari ini jam 16.30");
        assert!(!s.passed);
        let s = summary(Lang::En, Repeat::Once, mon, hm(16, 30));
        assert_eq!(s.plain(), "Once, today at 16:30 · in 4 hours");
        assert_eq!(s.bold, "today at 16:30");
        // A day or more away: no countdown.
        let tue = date(2026, 10, 27);
        assert_eq!(
            summary(Lang::Id, Repeat::Once, tue, hm(16, 30)).plain(),
            "Sekali, besok jam 16.30"
        );
        assert_eq!(
            summary(Lang::En, Repeat::Once, tue, hm(16, 30)).plain(),
            "Once, tomorrow at 16:30"
        );
        let wed = date(2026, 10, 28);
        let s = summary(Lang::Id, Repeat::Once, wed, hm(16, 30));
        assert_eq!(s.plain(), "Sekali, Rab, 28 Okt jam 16.30");
        assert_eq!(s.bold, "Rab, 28 Okt jam 16.30");
        assert_eq!(
            summary(Lang::En, Repeat::Once, wed, hm(16, 30)).plain(),
            "Once, Wed, 28 Oct at 16:30"
        );
        let next_year = date(2027, 1, 5);
        assert_eq!(
            summary(Lang::Id, Repeat::Once, next_year, hm(9, 0)).plain(),
            "Sekali, Sel, 5 Jan 2027 jam 09.00"
        );
        assert_eq!(
            summary(Lang::En, Repeat::Once, next_year, hm(9, 0)).plain(),
            "Once, Tue, 5 Jan 2027 at 09:00"
        );
        // Tomorrow, but within a day: hours.
        assert_eq!(
            summary(Lang::Id, Repeat::Once, tue, hm(8, 0)).tail,
            " · 19 jam lagi"
        );
    }

    #[test]
    fn one_off_countdowns_in_minutes_then_whole_hours() {
        let mon = date(2026, 10, 26);
        let tail = |lang, t| summary(lang, Repeat::Once, mon, t).tail;
        assert_eq!(tail(Lang::Id, hm(13, 0)), " · 30 menit lagi");
        assert_eq!(tail(Lang::En, hm(13, 0)), " · in 30 min");
        assert_eq!(tail(Lang::Id, hm(13, 29)), " · 59 menit lagi");
        assert_eq!(tail(Lang::Id, hm(13, 30)), " · 1 jam lagi");
        assert_eq!(tail(Lang::En, hm(13, 30)), " · in 1 hour");
        // Whole hours, rounded down.
        assert_eq!(tail(Lang::Id, hm(14, 29)), " · 1 jam lagi");
        assert_eq!(tail(Lang::En, hm(14, 30)), " · in 2 hours");
        // Under a minute still reads as one.
        let now = mon.and_hms_opt(12, 30, 30).unwrap();
        let s = reminder_summary(Lang::Id, Repeat::Once, mon, &[], hm(12, 31), now);
        assert_eq!(s.tail, " · 1 menit lagi");
        let s = reminder_summary(Lang::En, Repeat::Once, mon, &[], hm(12, 31), now);
        assert_eq!(s.tail, " · in 1 min");
    }

    #[test]
    fn past_one_offs_say_they_passed() {
        let sun = date(2026, 10, 25);
        let s = summary(Lang::Id, Repeat::Once, sun, hm(16, 30));
        assert_eq!(s.plain(), "Sekali, kemarin jam 16.30 · sudah lewat");
        assert!(s.passed);
        assert_eq!(
            summary(Lang::En, Repeat::Once, sun, hm(16, 30)).plain(),
            "Once, yesterday at 16:30 · already passed"
        );
        let mon = date(2026, 10, 26);
        assert_eq!(
            summary(Lang::Id, Repeat::Once, mon, hm(9, 0)).plain(),
            "Sekali, hari ini jam 09.00 · sudah lewat"
        );
        let s = summary(Lang::En, Repeat::Once, mon, hm(12, 30));
        assert_eq!(s.plain(), "Once, today at 12:30 · already passed");
        assert!(s.passed);
        // Earlier in the year, further back.
        assert_eq!(
            summary(Lang::Id, Repeat::Once, date(2026, 10, 20), hm(8, 0)).plain(),
            "Sekali, Sel, 20 Okt jam 08.00 · sudah lewat"
        );
    }

    #[test]
    fn repeating_summaries_say_when_they_ring_next() {
        let mon = date(2026, 10, 26);
        let s = summary(Lang::Id, Repeat::Daily, mon, hm(8, 0));
        assert_eq!(s.plain(), "Tiap hari jam 08.00 · berikutnya besok");
        assert_eq!(s.bold, "08.00");
        assert!(!s.passed);
        assert_eq!(
            summary(Lang::En, Repeat::Daily, mon, hm(8, 0)).plain(),
            "Every day at 08:00 · next tomorrow"
        );
        // Later today.
        assert_eq!(
            summary(Lang::Id, Repeat::Daily, mon, hm(16, 30)).plain(),
            "Tiap hari jam 16.30 · berikutnya hari ini"
        );
        // A daily start long past.
        assert_eq!(
            summary(Lang::En, Repeat::Daily, date(2026, 1, 3), hm(20, 0)).plain(),
            "Every day at 20:00 · next today"
        );
        // Weekly from a past Wednesday: the coming one.
        let wed = date(2026, 10, 21);
        let s = summary(Lang::Id, Repeat::Weekly, wed, hm(9, 0));
        assert_eq!(s.plain(), "Tiap Rabu jam 09.00 · berikutnya Rab, 28 Okt");
        assert_eq!(s.bold, "Rabu jam 09.00");
        assert_eq!(
            summary(Lang::En, Repeat::Weekly, wed, hm(9, 0)).plain(),
            "Every Wednesday at 09:00 · next Wed, 28 Oct"
        );
        // Weekly from a future Wednesday: that day.
        let later = date(2026, 11, 4);
        assert_eq!(
            summary(Lang::Id, Repeat::Weekly, later, hm(9, 0)).plain(),
            "Tiap Rabu jam 09.00 · berikutnya Rab, 4 Nov"
        );
        assert_eq!(
            summary(Lang::En, Repeat::Weekly, date(2026, 10, 27), hm(9, 0)).plain(),
            "Every Tuesday at 09:00 · next tomorrow"
        );
        // Weekly, earlier today: a week on.
        assert_eq!(
            summary(Lang::Id, Repeat::Weekly, mon, hm(9, 0)).plain(),
            "Tiap Senin jam 09.00 · berikutnya Sen, 2 Nov"
        );
    }

    #[test]
    fn next_occurrence_steps_from_the_start() {
        let now = monday_noon();
        let wed = date(2026, 10, 21);
        assert_eq!(
            next_occurrence(Repeat::Weekly, wed, &[], hm(9, 0), now),
            date(2026, 10, 28)
        );
        assert_eq!(
            next_occurrence(Repeat::Daily, wed, &[], hm(9, 0), now),
            date(2026, 10, 27)
        );
        assert_eq!(
            next_occurrence(Repeat::Daily, wed, &[], hm(13, 0), now),
            date(2026, 10, 26)
        );
        // At exactly now it has rung: the next one.
        assert_eq!(
            next_occurrence(Repeat::Daily, date(2026, 10, 26), &[], hm(12, 30), now),
            date(2026, 10, 27)
        );
        // Over a year end.
        let eve = at(date(2026, 12, 31), 23, 0);
        assert_eq!(
            next_occurrence(Repeat::Weekly, date(2026, 12, 3), &[], hm(22, 0), eve),
            date(2027, 1, 7)
        );
        // A start still ahead is itself the next.
        assert_eq!(
            next_occurrence(Repeat::Weekly, date(2026, 11, 4), &[], hm(9, 0), now),
            date(2026, 11, 4)
        );
    }

    #[test]
    fn multi_day_weekly_summaries_list_the_days() {
        use Weekday::{Fri, Mon, Wed};
        let now = monday_noon();
        let mon = date(2026, 10, 26);
        let s = reminder_summary(
            Lang::Id,
            Repeat::Weekly,
            mon,
            &[Mon, Wed, Fri],
            hm(9, 0),
            now,
        );
        assert_eq!(s.lead, "Tiap ");
        assert_eq!(s.bold, "Sen, Rab, Jum jam 09.00");
        assert_eq!(s.tail, " · berikutnya Rab, 28 Okt");
        assert!(!s.passed);
        let s = reminder_summary(
            Lang::En,
            Repeat::Weekly,
            mon,
            &[Fri, Mon, Wed],
            hm(9, 0),
            now,
        );
        assert_eq!(s.plain(), "Every Mon, Wed, Fri at 09:00 · next Wed, 28 Oct");
        assert_eq!(s.bold, "Mon, Wed, Fri at 09:00");
        // Later today it is still today's.
        let s = reminder_summary(Lang::Id, Repeat::Weekly, mon, &[Mon, Fri], hm(16, 0), now);
        assert_eq!(s.plain(), "Tiap Sen, Jum jam 16.00 · berikutnya hari ini");
        // One day reads as before, by its full name; the list beats the date.
        let s = reminder_summary(Lang::Id, Repeat::Weekly, mon, &[Mon], hm(9, 0), now);
        assert_eq!(s.plain(), "Tiap Senin jam 09.00 · berikutnya Sen, 2 Nov");
        let s = reminder_summary(Lang::En, Repeat::Weekly, mon, &[Fri], hm(9, 0), now);
        assert_eq!(s.plain(), "Every Friday at 09:00 · next Fri, 30 Oct");
        // Days mean nothing to a daily reminder.
        let s = reminder_summary(Lang::Id, Repeat::Daily, mon, &[Wed], hm(8, 0), now);
        assert_eq!(s.plain(), "Tiap hari jam 08.00 · berikutnya besok");
    }

    #[test]
    fn next_occurrence_follows_the_stores_rule() {
        use Weekday::{Fri, Mon, Thu, Wed};
        let now = monday_noon();
        let days = [Mon, Wed, Fri];
        let next = |start, t| next_occurrence(Repeat::Weekly, start, &days, t, now);
        // From today: past 09:00, so Wednesday; at 13:00 still today.
        assert_eq!(next(date(2026, 10, 26), hm(9, 0)), date(2026, 10, 28));
        assert_eq!(next(date(2026, 10, 26), hm(13, 0)), date(2026, 10, 26));
        // A start weeks back jumps to the coming one.
        assert_eq!(next(date(2026, 9, 2), hm(9, 0)), date(2026, 10, 28));
        // A start ahead on a day it skips: its first day from then.
        assert_eq!(next(date(2026, 11, 3), hm(9, 0)), date(2026, 11, 4));
        // The empty list is the start's weekday, as before.
        let thu = date(2026, 10, 22);
        assert_eq!(
            next_occurrence(Repeat::Weekly, thu, &[], hm(9, 0), now),
            next_occurrence(Repeat::Weekly, thu, &[Thu], hm(9, 0), now)
        );
        assert_eq!(
            next_occurrence(Repeat::Weekly, thu, &[], hm(9, 0), now),
            date(2026, 10, 29)
        );
        // A one-off is its own date.
        assert_eq!(next_occurrence(Repeat::Once, thu, &[], hm(9, 0), now), thu);
    }

    #[test]
    fn weekday_chips_and_quick_times_in_both_languages() {
        let week = [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ];
        let letters = |lang| week.map(|d| weekday_letter(lang, d)).join(" ");
        assert_eq!(letters(Lang::Id), "S S R K J S M");
        assert_eq!(letters(Lang::En), "M T W T F S S");
        assert_eq!(weekday_long(Lang::Id, Weekday::Wed), "Rabu");
        assert_eq!(weekday_short(Lang::En, Weekday::Sun), "Sun");
        assert_eq!(text(Lang::Id, Key::QuickHalfHour), "30 mnt lagi");
        assert_eq!(
            text(Lang::En, Key::QuickTomorrowMorning),
            "Tomorrow morning"
        );
    }

    #[test]
    fn run_times_and_results_in_both_languages() {
        use std::time::Duration;
        let ms = Duration::from_millis;
        assert_eq!(run_clock(ms(42_900)), "0:42");
        assert_eq!(run_clock(ms(725_000)), "12:05");
        assert_eq!(run_clock(ms(3_723_000)), "1:02:03");
        assert_eq!(cli_running(Lang::Id, ms(42_000)), "Berjalan · 0:42");
        assert_eq!(cli_running(Lang::En, ms(42_000)), "Running · 0:42");
        assert_eq!(run_duration(Lang::Id, ms(2_100)), "2,1 dtk");
        assert_eq!(run_duration(Lang::En, ms(2_100)), "2.1 s");
        assert_eq!(run_duration(Lang::Id, ms(149)), "0,1 dtk");
        assert_eq!(run_duration(Lang::Id, ms(59_960)), "1 mnt 0 dtk");
        assert_eq!(run_duration(Lang::Id, ms(72_000)), "1 mnt 12 dtk");
        assert_eq!(run_duration(Lang::En, ms(72_000)), "1 min 12 s");
        assert_eq!(run_duration(Lang::Id, ms(3_900_000)), "1 jam 5 mnt");
        assert_eq!(run_duration(Lang::En, ms(3_900_000)), "1 h 5 min");
        let took = ms(2_100);
        assert_eq!(
            cli_result(Lang::Id, Some(0), false, took),
            "selesai · 2,1 dtk"
        );
        assert_eq!(cli_result(Lang::En, Some(0), false, took), "done · 2.1 s");
        let took = ms(3_800);
        assert_eq!(
            cli_result(Lang::Id, Some(1), false, took),
            "exit 1 · 3,8 dtk"
        );
        assert_eq!(cli_result(Lang::En, Some(1), false, took), "exit 1 · 3.8 s");
        assert_eq!(
            cli_result(Lang::Id, Some(1), false, Duration::ZERO),
            "exit 1"
        );
        assert_eq!(cli_result(Lang::Id, None, false, Duration::ZERO), "exit ?");
        assert_eq!(
            cli_result(Lang::Id, Some(1), true, took),
            "dihentikan · 3,8 dtk"
        );
        assert_eq!(cli_result(Lang::En, Some(1), true, took), "stopped · 3.8 s");
    }

    #[test]
    fn saved_notice_hint_exists_in_both_languages() {
        assert_eq!(text(Lang::Id, Key::ShotClickToOpen), "klik untuk buka");
        assert_eq!(text(Lang::En, Key::ShotClickToOpen), "click to open");
    }

    #[test]
    fn screenshot_texts() {
        use crate::screenshot::FileOutcome;
        assert_eq!(
            shot_size(Lang::Id, 1280, 720, 250_880),
            "1280 × 720 · 245 KB"
        );
        assert_eq!(
            shot_size(Lang::Id, 3840, 2160, 1_887_437),
            "3840 × 2160 · 1,8 MB"
        );
        assert_eq!(
            shot_size(Lang::En, 3840, 2160, 1_887_437),
            "3840 × 2160 · 1.8 MB"
        );
        assert_eq!(shot_size(Lang::En, 2, 2, 90), "2 × 2 · 1 KB");
        assert_eq!(
            shot_deleted(Lang::Id, FileOutcome::Recycled, true),
            ("Dihapus", "clipboard dikosongkan".to_owned())
        );
        assert_eq!(
            shot_deleted(Lang::Id, FileOutcome::Recycled, false).1,
            "dipindah ke Recycle Bin · clipboard tidak diubah"
        );
        assert_eq!(
            shot_deleted(Lang::En, FileOutcome::NoFile, true).1,
            "no file found · clipboard cleared"
        );
        assert_eq!(
            shot_deleted(Lang::En, FileOutcome::Failed, false),
            (
                "Couldn't delete",
                "the file is still there · clipboard unchanged".to_owned()
            )
        );
        assert_eq!(seconds_short(Lang::Id, 8), "8 dtk");
        assert_eq!(seconds_short(Lang::En, 8), "8 s");
    }

    #[test]
    fn file_sizes_and_counts_read_well_in_both_languages() {
        let kb = 1024;
        for lang in [Lang::Id, Lang::En] {
            assert_eq!(search_file_size(lang, 0), "1 KB", "one at the least");
            assert_eq!(search_file_size(lang, 1), "1 KB");
            assert_eq!(search_file_size(lang, 245 * kb), "245 KB");
            assert_eq!(search_file_size(lang, 245 * kb + 100), "245 KB", "rounded");
            assert_eq!(search_file_size(lang, 1023 * kb), "1023 KB");
            assert_eq!(search_file_size(lang, 512 * MIB), "512 MB");
            assert_eq!(search_file_size(lang, 3 * GIB), format_size(lang, 3 * GIB));
        }
        // From a megabyte one decimal, in the language's mark.
        assert_eq!(search_file_size(Lang::Id, MIB), "1,0 MB");
        assert_eq!(search_file_size(Lang::En, MIB * 5 / 2), "2.5 MB");
        assert_eq!(search_file_size(Lang::Id, MIB * 5 / 2), "2,5 MB");
        assert_eq!(
            search_file_size(Lang::En, 1024 * kb - 10),
            "1.0 MB",
            "no 1024 KB"
        );
        assert_eq!(search_file_size(Lang::En, 9 * MIB), "9.0 MB");
        assert_eq!(search_file_size(Lang::En, 10 * MIB), "10 MB");

        assert_eq!(search_folder_items(Lang::Id, 1), "1 item");
        assert_eq!(search_folder_items(Lang::Id, 12), "12 item");
        assert_eq!(search_folder_items(Lang::En, 1), "1 item");
        assert_eq!(search_folder_items(Lang::En, 12), "12 items");
        assert_eq!(search_folder_items(Lang::En, 0), "0 items");
        assert_eq!(search_folder_items(Lang::En, 999), "999 items");
        assert_eq!(search_folder_items(Lang::En, 1000), "999+ items");
        assert_eq!(search_folder_items(Lang::Id, 5000), "999+ item");
    }

    #[test]
    fn a_day_label_reads_today_yesterday_or_the_date() {
        let d = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
        let today = d(11, 8);
        for (lang, want) in [
            (Lang::Id, ["Hari ini", "Kemarin", "7 Nov"]),
            (Lang::En, ["Today", "Yesterday", "7 Nov"]),
        ] {
            let got = [
                rec_day_label(lang, d(11, 8), today),
                rec_day_label(lang, d(11, 7), today),
                rec_day_label(lang, d(11, 7), d(11, 9)),
            ];
            assert_eq!(got, want.map(String::from), "{lang:?}");
            for g in &got {
                assert_no_leak(lang, "rec_day_label", g);
            }
        }
    }

    #[test]
    fn every_key_has_text_in_both_languages() {
        assert!(Key::ALL.len() > 50);
        for &key in Key::ALL {
            for lang in [Lang::Id, Lang::En] {
                let s = text(lang, key);
                assert!(!s.trim().is_empty(), "{key:?} {lang:?}");
                assert_eq!(s, s.trim(), "{key:?} {lang:?} has stray blanks");
            }
        }
    }

    #[test]
    fn sticky_on_desktop_tip_in_both_languages() {
        assert_eq!(text(Lang::Id, Key::StickyOnDesktopTip), "Tempel ke desktop");
        assert_eq!(text(Lang::En, Key::StickyOnDesktopTip), "Stick to desktop");
    }

    /// Common Indonesian words, never in English text.
    const INDONESIAN: &[&str] = &[
        "ada",
        "acara",
        "atau",
        "atas",
        "baru",
        "batal",
        "baterai",
        "belum",
        "berikutnya",
        "berjalan",
        "besok",
        "bisa",
        "buka",
        "bulan",
        "catatan",
        "cek",
        "dan",
        "dari",
        "di",
        "dengan",
        "dihentikan",
        "dtk",
        "dulu",
        "fokus",
        "gagal",
        "gaya",
        "hapus",
        "hari",
        "ini",
        "istirahat",
        "itu",
        "izin",
        "jalankan",
        "jam",
        "jeda",
        "kalender",
        "ke",
        "keluar",
        "kembali",
        "kemarin",
        "klik",
        "lagi",
        "lewat",
        "lihat",
        "lirik",
        "malam",
        "menit",
        "minggu",
        "mnt",
        "mulai",
        "pagi",
        "pengaturan",
        "pengingat",
        "perintah",
        "pintasan",
        "saat",
        "sekali",
        "selesai",
        "sesi",
        "setelah",
        "siap",
        "simpan",
        "sisa",
        "sore",
        "sudah",
        "tambah",
        "tanggal",
        "tanpa",
        "terhubung",
        "tiap",
        "tidak",
        "tutup",
        "ulangi",
        "untuk",
        "warna",
        "yang",
    ];

    /// Common English words, never in Indonesian text.
    const ENGLISH: &[&str] = &[
        "add",
        "and",
        "are",
        "at",
        "back",
        "break",
        "cancel",
        "check",
        "click",
        "close",
        "connect",
        "date",
        "day",
        "delete",
        "done",
        "edit",
        "end",
        "event",
        "events",
        "failed",
        "for",
        "from",
        "is",
        "minutes",
        "month",
        "more",
        "new",
        "next",
        "not",
        "note",
        "notes",
        "of",
        "open",
        "ready",
        "reminder",
        "reminders",
        "repeat",
        "save",
        "saved",
        "settings",
        "show",
        "start",
        "stop",
        "the",
        "time",
        "title",
        "to",
        "today",
        "tomorrow",
        "week",
        "with",
        "yesterday",
        "you",
        "your",
    ];

    /// Words both languages use as they are: brand and product names, loans
    /// the Indonesian UI keeps ("Edit acara", the "Notes" page, the CLI's
    /// "Stop"), key names and units. "edit", "notes" and "stop" are also in
    /// ENGLISH, so by design they are never flagged in either language.
    const SHARED_WORDS: &[&str] = &[
        "bata",
        "claude",
        "cli",
        "cmd",
        "edit",
        "github",
        "google",
        "notch",
        "notes",
        "ok",
        "pill",
        "pomodoro",
        "powershell",
        "readme",
        "screenshot",
        "spotify",
        "stop",
        "windows",
    ];

    /// Brand names written the same in both languages.
    const SHARED_PHRASES: &[&str] = &["Google Calendar"];

    /// "notes.json.bak", "lrclib.net", "D:\proj\a.json": a dot with a letter
    /// after it.
    fn is_file_name(chunk: &str) -> bool {
        chunk
            .char_indices()
            .any(|(i, c)| c == '.' && chunk[i + 1..].starts_with(char::is_alphabetic))
    }

    /// The words of `s` (HTML tags, file names and the shared phrases taken
    /// out), lowercase, that are in `banned` and not shared.
    fn leaked(s: &str, banned: &[&str]) -> Vec<String> {
        let mut plain = String::new();
        let mut in_tag = false;
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                // Only "<a" and "</" open a tag; a lone "<" ("< 5 mnt") is text.
                '<' if chars.peek().is_some_and(|n| n.is_alphabetic() || *n == '/') => {
                    in_tag = true
                }
                '>' if in_tag => in_tag = false,
                _ if !in_tag => plain.push(c),
                _ => {}
            }
        }
        for phrase in SHARED_PHRASES {
            plain = plain.replace(phrase, " ");
        }
        plain
            .split_whitespace()
            .filter(|chunk| !is_file_name(chunk))
            .flat_map(|chunk| chunk.split(|c: char| !c.is_alphanumeric()))
            .map(str::to_lowercase)
            .filter(|w| banned.contains(&w.as_str()) && !SHARED_WORDS.contains(&w.as_str()))
            .collect()
    }

    const ALL_PERIODS: [DayPeriod; 5] = [
        DayPeriod::Morning,
        DayPeriod::Midday,
        DayPeriod::Afternoon,
        DayPeriod::Evening,
        DayPeriod::Night,
    ];

    fn assert_no_leak(lang: Lang, what: &str, s: &str) {
        let banned = match lang {
            Lang::Id => ENGLISH,
            Lang::En => INDONESIAN,
        };
        let words = leaked(s, banned);
        assert!(words.is_empty(), "{what} {lang:?} {s:?} has {words:?}");
    }

    #[test]
    fn leak_check_finds_words_on_their_boundaries() {
        assert_eq!(leaked("Save the note", ENGLISH), ["save", "the", "note"]);
        assert_eq!(leaked("Tidak ada", INDONESIAN), ["tidak", "ada"]);
        // Inside a longer word is no match; nor is a shared word or phrase,
        // a file name or a tag.
        assert_eq!(leaked("Theme · Saved?", ENGLISH), ["saved"]);
        assert!(leaked("Sedia", INDONESIAN).is_empty());
        // A lone "<" is text, not a tag: what follows it is still checked.
        assert_eq!(leaked("< 5 menit the", ENGLISH), ["the"]);
        assert!(leaked("Edit Notes · Pengaturan Windows", ENGLISH).is_empty());
        assert!(leaked("Buka Google Calendar", ENGLISH).is_empty());
        // An apostrophe ends a word, like any other mark.
        assert_eq!(leaked("Today's", ENGLISH), ["today"]);
        assert_eq!(leaked("don't Settings' isn't", ENGLISH), ["settings"]);
        assert!(leaked("reminders.json tidak terbaca.", ENGLISH).is_empty());
        assert!(leaked("<title>x</title>", ENGLISH).is_empty());
    }

    #[test]
    fn tr_running_reads_in_both_languages() {
        use std::time::Duration;
        let six = Some(Duration::from_secs(6 * 60 + 20));
        assert_eq!(tr_running(Lang::Id, 42, six), "42% \u{b7} sekitar 6 mnt lagi");
        assert_eq!(tr_running(Lang::En, 42, six), "42% \u{b7} about 6 min left");
        assert_eq!(tr_running(Lang::Id, 3, None), "3%");
        assert_eq!(tr_running(Lang::En, 0, Some(Duration::from_secs(45))), "0% \u{b7} about 45 s left");
    }

    #[test]
    fn a_transcript_pill_reads_in_both_languages() {
        use crate::model::RowTranscript as M;
        let all = [
            (M::Off, "Transkrip", "Transcribe"),
            (M::None, "Transkrip", "Transcribe"),
            (M::Queued, "Antre", "Queued"),
            (M::Waiting, "Menunggu model", "Waiting for model"),
            (M::Running(42), "42%", "42%"),
            (M::Done, "Lihat", "View"),
            (M::Failed, "Gagal", "Failed"),
        ];
        for (mark, id, en) in all {
            assert_eq!(tr_pill(Lang::Id, mark), id, "{mark:?}");
            assert_eq!(tr_pill(Lang::En, mark), en, "{mark:?}");
        }
    }

    #[test]
    fn a_recording_length_reads_in_both_languages() {
        use std::time::Duration;
        let secs = Duration::from_secs;
        assert_eq!(rec_duration(Lang::Id, secs(0)), "0 dtk");
        assert_eq!(rec_duration(Lang::Id, secs(45)), "45 dtk");
        assert_eq!(rec_duration(Lang::Id, secs(42 * 60 + 59)), "42 mnt");
        assert_eq!(rec_duration(Lang::Id, secs(3600 + 5 * 60)), "1 j 05 mnt");
        assert_eq!(rec_duration(Lang::En, secs(45)), "45 s");
        assert_eq!(rec_duration(Lang::En, secs(18 * 60)), "18 min");
        assert_eq!(rec_duration(Lang::En, secs(2 * 3600 + 30 * 60)), "2 h 30 min");
        for lang in [Lang::Id, Lang::En] {
            for d in [secs(5), secs(600), secs(7200)] {
                assert_no_leak(lang, "rec_duration", &rec_duration(lang, d));
            }
        }
    }

    #[test]
    fn rec_copy_reads_right_in_both_languages() {
        let pairs = [
            (Key::RecRecordLabel, "Rekam", "Record"),
            (Key::RecNow, "Sekarang", "Now"),
            (Key::RecMuted, "Dibisukan", "Muted"),
            (Key::ScreenDiscardAsk, "Buang video ini?", "Throw this video away?"),
            (Key::ScreenDiscardKeep, "Simpan", "Keep"),
            (Key::RecQuality, "Kualitas", "Quality"),
            (
                Key::ScreenIdleHint,
                "Layar, jendela atau area: dipilih setelah tekan Rekam",
                "Screen, window or area: you pick it after Record",
            ),
        ];
        for (key, id, en) in pairs {
            assert_eq!(text(Lang::Id, key), id, "{key:?}");
            assert_eq!(text(Lang::En, key), en, "{key:?}");
        }
        assert_eq!(rec_more(Lang::Id, 9), "9 lagi di folder");
        assert_eq!(rec_more(Lang::En, 9), "9 more in the folder");
        assert_eq!(rec_muted(Lang::Id, None), "Dibisukan");
        assert_eq!(rec_muted(Lang::Id, Some("Ctrl+Alt+M")), "Dibisukan \u{b7} Ctrl+Alt+M");
        assert_eq!(rec_muted(Lang::En, None), "Muted");
        assert_eq!(rec_muted(Lang::En, Some("Ctrl+Alt+M")), "Muted \u{b7} Ctrl+Alt+M");
        assert_eq!(rec_empty_hint(Lang::Id, None), "Tekan Rekam");
        assert_eq!(
            rec_empty_hint(Lang::Id, Some("Ctrl+Alt+R")),
            "Tekan Rekam, atau Ctrl+Alt+R dari mana aja",
        );
        assert_eq!(rec_empty_hint(Lang::En, None), "Press Record");
        assert_eq!(
            rec_empty_hint(Lang::En, Some("Ctrl+Alt+R")),
            "Press Record, or Ctrl+Alt+R from anywhere",
        );
        assert_eq!(rec_quality_label(Lang::Id, VideoQuality::P1080, 30), "1080p \u{b7} 30 fps");
        assert_eq!(rec_quality_label(Lang::En, VideoQuality::P1080, 60), "1080p \u{b7} 60 fps");
        assert_eq!(rec_quality_label(Lang::Id, VideoQuality::Original, 30), "Asli \u{b7} 30 fps");
        assert_eq!(
            rec_quality_label(Lang::En, VideoQuality::Original, 30),
            "Original \u{b7} 30 fps",
        );
        for lang in [Lang::Id, Lang::En] {
            for &key in &[Key::RecRecordLabel, Key::RecNow, Key::RecMuted, Key::ScreenDiscardAsk, Key::ScreenDiscardKeep, Key::RecQuality, Key::ScreenIdleHint, Key::RecDeviceMissing] {
                assert_no_leak(lang, &format!("{key:?}"), text(lang, key));
                assert!(!text(lang, key).contains('\u{2014}'), "{key:?} {lang:?}");
            }
            let formatted = [
                rec_more(lang, 9),
                rec_muted(lang, None),
                rec_muted(lang, Some("Ctrl+Alt+M")),
                rec_empty_hint(lang, None),
                rec_empty_hint(lang, Some("Ctrl+Alt+R")),
                rec_quality_label(lang, VideoQuality::P1080, 30),
                rec_quality_label(lang, VideoQuality::Original, 30),
            ];
            for s in &formatted {
                assert_no_leak(lang, "formatted", s);
                assert!(!s.contains('\u{2014}'), "{s:?} {lang:?}");
            }
        }
    }

    #[test]
    fn the_zoom_label_is_a_level_with_the_languages_separator() {
        assert_eq!(zoom_label(Lang::Id, 2.0), "2\u{D7}");
        assert_eq!(zoom_label(Lang::Id, 2.5), "2,5\u{D7}");
        assert_eq!(zoom_label(Lang::En, 2.5), "2.5\u{D7}");
        assert_eq!(zoom_label(Lang::En, 4.0), "4\u{D7}");
        assert_eq!(zoom_label(Lang::Id, 1.5), "1,5\u{D7}");
    }

    #[test]
    fn screen_recording_texts() {
        let table = [
            (Key::RecBusyTitle, "Rekaman lain sedang jalan", "Another recording is running"),
            (Key::RecBusyBody, "Hentikan dulu, lalu mulai lagi", "Stop it first, then start again"),
            (Key::HotkeyRowRecordScreen, "Rekam layar", "Record the screen"),
            (
                Key::HotkeyRowRecordScreenDesc,
                "Pilih layar, jendela atau area",
                "Pick a screen, window or area",
            ),
            (Key::HotkeyRowRecordZoom, "Zoom video rekaman", "Zoom the recording video"),
            (
                Key::HotkeyRowRecordZoomDesc,
                "Hanya aktif saat merekam layar",
                "Only while recording the screen",
            ),
            (Key::TrayScreenStart, "Rekam layar", "Record the screen"),
            (Key::TrayScreenStop, "Hentikan rekam layar", "Stop screen recording"),
            (Key::ScreenSavedTitle, "Video tersimpan", "Video saved"),
            (
                Key::ScreenWindowClosedTitle,
                "Jendela ditutup, video tersimpan",
                "Window closed, video saved",
            ),
            (
                Key::ScreenDisplayGoneTitle,
                "Layar terputus, video tersimpan",
                "Display disconnected, video saved",
            ),
            (Key::ScreenDiscardedTitle, "Video dibuang", "Video discarded"),
            (Key::ScreenFailedTitle, "Rekam layar gagal", "Screen recording failed"),
            (
                Key::ScreenWhyUnsupported,
                "Windows ini tidak bisa merekam layar",
                "This Windows cannot record the screen",
            ),
            (
                Key::ScreenWhyGone,
                "Jendela atau layarnya sudah tidak ada",
                "The window or screen is gone",
            ),
            (
                Key::ScreenWhyMinimized,
                "Jendelanya sedang diminimalkan. Buka dulu, lalu coba lagi",
                "The window is minimized. Restore it, then try again",
            ),
            (
                Key::ScreenWhyEncoder,
                "Pengode video tidak tersedia di PC ini",
                "No video encoder is available on this PC",
            ),
            (
                Key::ScreenWhyGpu,
                "Kartu grafis tidak bisa dipakai untuk merekam",
                "The graphics card cannot be used to record",
            ),
            (
                Key::ScreenWhyWrite,
                "File video tidak bisa ditulis",
                "The video file cannot be written",
            ),
            (
                Key::ScreenWhyOther,
                "Ada yang tidak beres saat merekam layar",
                "Something went wrong while recording the screen",
            ),
            (Key::ScreenLabel, "Layar", "Screen"),
            (Key::ScreenCountdownEsc, "Esc untuk batal", "Esc to cancel"),
            (Key::RecTabAudio, "Audio", "Audio"),
            (Key::RecTabScreen, "Layar", "Screen"),
            (
                Key::ScreenIdleHint,
                "Layar, jendela atau area: dipilih setelah tekan Rekam",
                "Screen, window or area: you pick it after Record",
            ),
            (Key::ScreenDiscard, "Buang", "Discard"),
            (Key::ScreenNoVideos, "Belum ada video", "No videos yet"),
            (Key::ScreenSettingsHighlight, "Sorot kursor", "Highlight the cursor"),
            (
                Key::ScreenSettingsHighlightDesc,
                "Lingkaran kuning di kursor \u{b7} Berlaku mulai rekaman berikutnya",
                "A yellow disc on the cursor \u{b7} Applies from the next recording",
            ),
            (Key::ScreenSettingsRipples, "Riak klik", "Click ripples"),
            (
                Key::ScreenSettingsRipplesDesc,
                "Riak melebar tiap klik \u{b7} Berlaku mulai rekaman berikutnya",
                "A ripple on every click \u{b7} Applies from the next recording",
            ),
        ];
        for (key, id, en) in table {
            assert_eq!(text(Lang::Id, key), id, "{key:?}");
            assert_eq!(text(Lang::En, key), en, "{key:?}");
        }
    }

    #[test]
    fn no_key_leaks_the_other_language() {
        for &key in Key::ALL {
            for lang in [Lang::Id, Lang::En] {
                assert_no_leak(lang, &format!("{key:?}"), text(lang, key));
            }
        }
    }

    #[test]
    fn no_formatted_text_leaks_the_other_language() {
        use std::time::Duration;
        let today = date(2026, 10, 26);
        let now = monday_noon();
        let days = [Weekday::Mon, Weekday::Wed];
        let path = r"D:\proj\a.json";
        for lang in [Lang::Id, Lang::En] {
            let mut all = vec![
                update_available(lang, "3.11.0"),
                update_menu_install(lang, "3.11.0"),
                update_downloading(lang, 42),
                update_failed(lang, text(lang, Key::UpdateErrChecksum)),
                update_up_to_date(lang, "3.10.0"),
                update_done(lang, "3.11.0"),
                update_installed(lang, "3.10.0"),
                focus_done_body(lang, 5),
                break_done_body(lang, 2, 4),
                session_of(lang, 1, 4),
                pomo_durations(lang, 25, 5, 15),
                phase_minutes(lang, text(lang, Key::Break), 5),
                tr_why_refused(lang, 404),
                tr_offer(lang, "Whisper large-v3-turbo", 555, 19),
                tr_used(lang, 610),
                tr_progress(220, 555),
                tr_running(lang, 42, Some(Duration::from_secs(6 * 60))),
                tr_running(lang, 7, None),
                phase_minutes(lang, text(lang, Key::Focus), 25),
                more_count(lang, 3),
                battery_body(lang, 18),
                config_copied_to(lang, "config.json.bak"),
                version_label(lang, "2.1.0"),
                cd_not_found(lang, r"D:\x"),
                cli_start_failed(lang, "0x80070002"),
                cli_running(lang, Duration::from_secs(42)),
                cli_moved_to(lang, r"D:\github\project-a"),
                clip_search_in(lang, 48),
                clip_search_in(lang, 1),
                clip_image_meta(lang, true, 1280, 720),
                clip_image_meta(lang, false, 640, 480),
                clip_files(lang, 3, "a.pdf"),
                clip_text_meta(lang, crate::clipboard::ClipKind::Text, 42),
                clip_text_meta(lang, crate::clipboard::ClipKind::Link, 42),
                clip_preview_size(1280, 720),
                save_failed_because(lang, text(lang, Key::ReasonOffline)),
                google_refused(lang, "403"),
                unreadable_reply(lang, 200),
                not_valid_json(lang, path, "EOF"),
                unknown_format(lang, path),
                oauth_page(lang, Key::OauthConnected),
                month_title(lang, today),
                day_title_long(lang, today),
                week_title(lang, today),
                editor_date(lang, today),
                day_date_short(lang, today),
                time_label(lang, hm(9, 30)),
            ];
            {
                use crate::transcribe::doc::{self, Doc};
                use crate::transcribe::output::{Segment, Speaker};
                let segments = (0..25)
                    .map(|i| Segment {
                        start_ms: i * 1000,
                        end_ms: i * 1000 + 900,
                        speaker: [Some(Speaker::Me), Some(Speaker::Others), None][i as usize % 3],
                        text: format!("ok {i}"),
                    })
                    .collect();
                let started = today.and_hms_opt(9, 30, 0).unwrap();
                let doc = Doc {
                    audio: r"D:\rec\a.m4a".into(),
                    md: r"D:\rec\a.md".into(),
                    title: "Sync".into(),
                    app: None,
                    started: Some(started),
                    duration_ms: Some(2_520_000),
                    model: Some("whisper-large-v3-turbo".into()),
                    language: Some("en".into()),
                    two_channel: true,
                    has_transcript: true,
                    segments,
                };
                all.push(doc::meta_line(&doc, lang));
                all.push(doc::plain_text(&doc, lang));
                all.push(doc::note_text(&doc, lang));
            }
            {
                // The saved-video notice of a screen recording, for each way
                // it can end and both bodies.
                use crate::app::screen_page::{Ended, no_sound_notice, video_saved_notice};
                use crate::recorder::Failure;
                for failure in [
                    Failure::NoDevice,
                    Failure::NoMediaFoundation,
                    Failure::NoAudio,
                    Failure::Other("x".into()),
                ] {
                    let n = no_sound_notice(lang, &failure);
                    all.push(n.title);
                    all.push(n.body);
                }
                use crate::model::TranscriptOffer;
                for ended in [Ended::Stopped, Ended::WindowClosed, Ended::DisplayGone] {
                    for offer in [TranscriptOffer::None, TranscriptOffer::Setup] {
                        let n = video_saved_notice(
                            lang,
                            std::path::PathBuf::new(),
                            Duration::from_secs(197),
                            ended,
                            offer,
                            None,
                        );
                        all.push(n.title);
                        all.push(n.body);
                    }
                }
            }
            all.push(greeting_periods_line(lang));
            for period in ALL_PERIODS {
                all.push(greeting_title(lang, period).into());
                all.push(greeting_period_name(lang, period).into());
                for (events, reminders) in [(0, 0), (1, 1), (3, 2), (2, 0), (0, 2)] {
                    all.push(greeting_body(
                        lang,
                        period,
                        GreetCounts { events, reminders },
                    ));
                }
            }
            for ms in [2_100, 72_000, 3_900_000] {
                all.push(run_duration(lang, Duration::from_millis(ms)));
            }
            for (code, killed) in [(Some(0), false), (Some(1), false), (None, true)] {
                all.push(cli_result(lang, code, killed, Duration::from_millis(3_800)));
            }
            for minutes in [30, 60, 90] {
                all.push(duration_label(lang, minutes));
            }
            for secs in [300, 21_660, 194_400] {
                all.push(uptime(lang, Duration::from_secs(secs)));
            }
            all.push(disk_free(lang, 47 * GIB, 338 * GIB));
            all.push(ram_usage(lang, 14 * GIB, 31 * GIB, 44));
            all.push(cpu_cores(lang, 8));
            all.push(disk_low_title(lang, 'C', 9 * GIB));
            all.push(ram_high_title(lang, 93));
            {
                let all_fit = |_: &str| true;
                let note = CleanNote {
                    drive: Some(('C', 49 * GIB)),
                    skipped: 3,
                    browsers: &["Chrome", "Edge"],
                    admin_skipped: true,
                };
                all.push(clean_done_body(lang, &note, &all_fit));
                all.push(clean_done_body(lang, &note, &|s| s.len() < 40));
                // Every wording, the shortest ones too.
                let none_fit = |_: &str| false;
                for fit in [
                    &all_fit as &dyn Fn(&str) -> bool,
                    &|s: &str| s.len() < 26,
                    &none_fit,
                ] {
                    all.push(mole_done_title(lang, 2 * GIB, fit));
                    all.push(mole_done_body(lang, 'C', true, fit));
                    all.push(clean_done_body(lang, &note, fit));
                }
                all.push(mole_failed_title(lang, Some(1)));
                all.push(mole_failed_title(lang, None));
                all.push(mole_failed_body(lang, 0));
                all.push(mole_failed_body(lang, GIB));
                for (v, tested) in [
                    (Some("1.30.0"), true),
                    (Some("1.31.0"), false),
                    (None, false),
                ] {
                    all.push(mole_intro(lang, v, tested));
                }
                all.push(mole_preview(lang, Some(GIB)));
                all.push(mole_preview(lang, None));
                all.push(mole_total_items(lang, GIB, 29));
                all.push(mole_result(lang, GIB, Some(('C', 47 * GIB, 49 * GIB))));
            }
            {
                use crate::cli_folders::Ago;
                for ago in [
                    Ago::JustNow,
                    Ago::Minutes(2),
                    Ago::Hours(3),
                    Ago::Yesterday,
                    Ago::On(today),
                ] {
                    all.push(folder_ago(lang, ago));
                }
            }
            for d in today.iter_days().take(7) {
                all.push(day_long(lang, d).to_owned());
                all.push(day_short(lang, d).to_owned());
            }
            for repeat in [Repeat::Once, Repeat::Daily, Repeat::Weekly] {
                for d in [today - chrono::Days::new(1), today, date(2026, 10, 28)] {
                    all.push(reminder_date_chip(lang, repeat, d, today));
                    for t in [hm(9, 0), hm(13, 0), hm(16, 30)] {
                        all.push(reminder_summary(lang, repeat, d, &[], t, now).plain());
                        all.push(reminder_summary(lang, repeat, d, &days, t, now).plain());
                    }
                }
            }
            for s in &all {
                assert_no_leak(lang, "formatted", s);
            }
        }
    }

    #[test]
    fn cleaner_texts_in_both_languages() {
        let gb = |x: f64| (x * GIB as f64).round() as u64;
        assert_eq!(clean_size(Lang::Id, gb(0.1)), "0,1 GB");
        assert_eq!(clean_size(Lang::Id, gb(1.8)), "1,8 GB");
        assert_eq!(clean_size(Lang::En, gb(2.3)), "2.3 GB");
        assert_eq!(clean_size(Lang::Id, 38 * MIB), "38 MB");
        assert_eq!(clean_size(Lang::En, 0), "0 MB");
        assert_eq!(clean_size(Lang::Id, gb(1500.0)), "1,5 TB");
        assert_eq!(clean_picked(Lang::Id, gb(2.3)), "Dipilih 2,3 GB");
        assert_eq!(clean_go(Lang::Id, gb(2.3)), "Bersihkan 2,3 GB");
        assert_eq!(clean_go(Lang::En, gb(2.3)), "Clean 2.3 GB");
        assert_eq!(clean_confirm(Lang::Id, gb(2.3)), "Yakin? Hapus 2,3 GB");
        assert_eq!(clean_confirm(Lang::En, gb(2.3)), "Sure? Delete 2.3 GB");
        assert_eq!(clean_freed(Lang::Id, gb(2.3)), "2,3 GB dibebaskan");
        assert_eq!(clean_freed(Lang::En, gb(2.3)), "2.3 GB freed");
        let note = CleanNote {
            drive: Some(('C', gb(49.5))),
            skipped: 3,
            ..CleanNote::default()
        };
        let all = |_: &str| true;
        // Room enough: the full wording.
        assert_eq!(
            clean_done_body(Lang::Id, &note, &all),
            "Disk C: sekarang sisa 49,5 GB · 3 item dilewati (sedang dipakai)"
        );
        assert_eq!(
            clean_done_body(Lang::En, &note, &all),
            "Disk C: now 49.5 GB free · 3 items skipped (in use)"
        );
        // Less room: every part shorter before any part goes.
        let short = |s: &str| s.chars().count() <= 40;
        assert_eq!(
            clean_done_body(Lang::Id, &note, &short),
            "C: sisa 49,5 GB · 3 item dilewati"
        );
        assert_eq!(
            clean_done_body(Lang::En, &note, &short),
            "C: 49.5 GB free · 3 items skipped"
        );
        let more = CleanNote {
            drive: None,
            skipped: 1,
            browsers: &["Chrome", "Edge"],
            admin_skipped: true,
        };
        assert_eq!(
            clean_done_body(Lang::En, &more, &all),
            "1 item skipped (in use) · Chrome, Edge open · Windows Update skipped"
        );
        assert_eq!(clean_done_body(Lang::Id, &CleanNote::default(), &all), "");
        // Nothing fits: the first part, shortest, never an empty body.
        assert_eq!(
            clean_done_body(Lang::Id, &note, &|_| false),
            "C: sisa 49,5 GB"
        );
        // What does not fit goes, the least important first; a long list
        // of browsers becomes a count.
        let many = CleanNote {
            drive: Some(('C', gb(49.5))),
            skipped: 123,
            browsers: &["Chrome", "Edge", "Firefox"],
            admin_skipped: true,
        };
        assert_eq!(
            clean_done_body(Lang::Id, &many, &short),
            "C: sisa 49,5 GB · 123 item dilewati"
        );
        let wider = |s: &str| s.chars().count() <= 58;
        assert_eq!(
            clean_done_body(Lang::Id, &many, &wider),
            "C: sisa 49,5 GB · 123 item dilewati · 3 browser terbuka"
        );
        // Mole's texts.
        assert_eq!(
            mole_done_title(Lang::Id, gb(2.4), &all),
            "Mole selesai · 2,4 GB dibebaskan"
        );
        assert_eq!(
            mole_done_title(Lang::En, gb(2.4), &all),
            "Mole done · 2.4 GB freed"
        );
        let title_room = |s: &str| s.chars().count() <= 24;
        assert_eq!(
            mole_done_title(Lang::Id, gb(2.4), &title_room),
            "Mole · 2,4 GB dibebaskan"
        );
        assert_eq!(
            mole_done_title(Lang::En, gb(2.4), &|_| false),
            "2.4 GB freed"
        );
        assert_eq!(
            mole_done_body(Lang::Id, 'C', false, &all),
            "Diukur dari sisa disk C:"
        );
        assert_eq!(
            mole_done_body(Lang::En, 'C', true, &all),
            "Measured from free space on C: · System (admin) skipped"
        );
        assert_eq!(
            mole_done_body(Lang::Id, 'C', true, &|s| s.chars().count() <= 30),
            "Diukur di C: · Sistem dilewati"
        );
        assert_eq!(mole_failed_title(Lang::Id, Some(1)), "Mole gagal · kode 1");
        assert_eq!(mole_failed_title(Lang::En, None), "Mole stopped");
        assert_eq!(mole_failed_body(Lang::Id, 0), "Lihat log di Bersihkan");
        assert_eq!(
            mole_failed_body(Lang::En, gb(0.4)),
            "0.4 GB freed before it stopped"
        );
        assert_eq!(
            mole_intro(Lang::Id, Some("1.30.0"), true),
            "Mole 1.30.0 memilih sendiri apa yang dibersihkan · hapus permanen"
        );
        assert_eq!(mole_preview(Lang::Id, Some(gb(2.4))), "Pratinjau: ± 2,4 GB");
        assert_eq!(mole_preview(Lang::En, None), "Preview: see log");
        assert_eq!(
            mole_total_items(Lang::Id, gb(2.4), 29),
            "± 2,4 GB · 29 item"
        );
        assert_eq!(
            mole_result(Lang::Id, gb(2.4), Some(('C', gb(47.2), gb(49.6)))),
            "2,4 GB dibebaskan · C: 47,2 GB \u{2192} 49,6 GB"
        );
        assert_eq!(
            text(Lang::Id, clean_cat(CleanCat::TempLogs).0),
            "File sementara & log"
        );
        assert_eq!(
            text(Lang::En, clean_cat(CleanCat::WindowsUpdate).1),
            "Needs admin permission"
        );
        assert_eq!(
            text(Lang::Id, Key::CleanCredit),
            "Ukuran dari pemindaian · daftar lokasi: Mole Windows (MIT)"
        );
    }

    #[test]
    fn system_page_texts_in_both_languages() {
        use std::time::Duration;
        let gb = |x: f64| (x * GIB as f64).round() as u64;
        assert_eq!(format_size(Lang::Id, gb(47.2)), "47,2 GB");
        assert_eq!(format_size(Lang::En, gb(47.2)), "47.2 GB");
        assert_eq!(format_size(Lang::Id, gb(3.1)), "3,1 GB");
        assert_eq!(format_size(Lang::Id, 512 * MIB), "512 MB");
        assert_eq!(format_size(Lang::En, 0), "0 MB");
        assert_eq!(format_size(Lang::Id, GIB - 1), "1,0 GB");
        assert_eq!(format_size(Lang::En, 1023 * MIB), "1023 MB");
        assert_eq!(format_size(Lang::En, gb(999.96)), "1.0 TB");
        assert_eq!(format_size(Lang::En, gb(999.9)), "999.9 GB");
        assert_eq!(format_size(Lang::Id, gb(1863.0)), "1,8 TB");
        assert_eq!(
            disk_free(Lang::Id, gb(47.2), gb(338.4)),
            "sisa 47,2 GB dari 338,4 GB"
        );
        assert_eq!(
            disk_free(Lang::En, gb(47.2), gb(338.4)),
            "47.2 GB free of 338.4 GB"
        );
        assert_eq!(
            ram_usage(Lang::Id, gb(14.2), gb(31.8), 44),
            "14,2 / 31,8 GB · 44%"
        );
        assert_eq!(
            ram_usage(Lang::En, gb(14.2), gb(31.8), 44),
            "14.2 / 31.8 GB · 44%"
        );
        let up = Duration::from_secs(6 * 3600 + 60 + 30);
        assert_eq!(uptime(Lang::Id, up), "Uptime 6 j 1 m");
        assert_eq!(uptime(Lang::En, up), "Uptime 6 h 1 m");
        let days = Duration::from_secs(2 * 86_400 + 6 * 3600 + 59 * 60);
        assert_eq!(uptime(Lang::Id, days), "Uptime 2 hr 6 j");
        assert_eq!(uptime(Lang::En, days), "Uptime 2 d 6 h");
        assert_eq!(uptime(Lang::En, Duration::from_secs(720)), "Uptime 12 m");
        assert_eq!(cpu_cores(Lang::Id, 8), "CPU · 8 inti");
        assert_eq!(cpu_cores(Lang::En, 8), "CPU · 8 cores");
        assert_eq!(
            disk_low_title(Lang::Id, 'C', gb(9.6)),
            "Disk C: tinggal 9,6 GB"
        );
        assert_eq!(
            disk_low_title(Lang::En, 'C', gb(9.6)),
            "Disk C: 9.6 GB left"
        );
        assert_eq!(ram_high_title(Lang::Id, 93), "RAM terpakai 93%");
        assert_eq!(ram_high_title(Lang::En, 93), "RAM 93% used");
        assert_eq!(text(Lang::Id, Key::System), "Sistem");
        assert_eq!(text(Lang::En, Key::SystemSoon), "soon");
    }

    #[test]
    fn greeting_periods_line_is_built_from_the_greeting_code() {
        assert_eq!(
            greeting_periods_line(Lang::Id),
            "Pagi 04-10 Selamat pagi · Siang 11-14 Selamat siang · Sore 15-17 Selamat sore · Malam 18-23 Selamat malam · Dini hari 00-03 Masih begadang?"
        );
        assert_eq!(
            greeting_periods_line(Lang::En),
            "Morning 04-10 Good morning · Noon 11-14 Good afternoon · Afternoon 15-17 Good afternoon · Evening 18-23 Good evening · Late night 00-03 Up late?"
        );
    }

    #[test]
    fn greeting_in_both_languages() {
        assert_eq!(text(Lang::Id, Key::GoodMorning), "Selamat pagi");
        assert_eq!(text(Lang::En, Key::GoodMorning), "Good morning");
        let c = |events, reminders| GreetCounts { events, reminders };
        let body = |lang, p, e, r| greeting_body(lang, p, c(e, r));
        use DayPeriod::*;
        for (p, id, en) in [
            (Morning, "Selamat pagi", "Good morning"),
            (Midday, "Selamat siang", "Good afternoon"),
            (Afternoon, "Selamat sore", "Good afternoon"),
            (Evening, "Selamat malam", "Good evening"),
            (Night, "Masih begadang?", "Up late?"),
        ] {
            assert_eq!(greeting_title(Lang::Id, p), id, "{p:?}");
            assert_eq!(greeting_title(Lang::En, p), en, "{p:?}");
        }
        // Morning: today's.
        assert_eq!(
            body(Lang::Id, Morning, 3, 2),
            "3 acara dan 2 pengingat hari ini"
        );
        assert_eq!(body(Lang::Id, Morning, 0, 2), "2 pengingat hari ini");
        assert_eq!(body(Lang::Id, Morning, 3, 0), "3 acara hari ini");
        assert_eq!(body(Lang::Id, Morning, 0, 0), "Hari ini belum ada acara");
        assert_eq!(
            body(Lang::En, Morning, 3, 2),
            "3 events and 2 reminders today"
        );
        assert_eq!(
            body(Lang::En, Morning, 1, 1),
            "1 event and 1 reminder today"
        );
        assert_eq!(body(Lang::En, Morning, 0, 2), "2 reminders today");
        assert_eq!(body(Lang::En, Morning, 1, 0), "1 event today");
        assert_eq!(body(Lang::En, Morning, 0, 0), "Nothing planned today");
        // Midday and Afternoon: what is left today.
        for p in [Midday, Afternoon] {
            assert_eq!(
                body(Lang::Id, p, 2, 1),
                "Masih 2 acara dan 1 pengingat hari ini"
            );
            assert_eq!(body(Lang::Id, p, 0, 1), "Masih 1 pengingat hari ini");
            assert_eq!(body(Lang::Id, p, 2, 0), "Masih 2 acara hari ini");
            assert_eq!(body(Lang::Id, p, 0, 0), "Sisa hari ini kosong");
            assert_eq!(
                body(Lang::En, p, 2, 1),
                "2 events and 1 reminder left today"
            );
            assert_eq!(body(Lang::En, p, 0, 2), "2 reminders left today");
            assert_eq!(body(Lang::En, p, 1, 0), "1 event left today");
            assert_eq!(body(Lang::En, p, 0, 0), "Nothing left today");
        }
        // Evening: tomorrow's events; reminders are not counted.
        assert_eq!(body(Lang::Id, Evening, 2, 0), "Besok 2 acara");
        assert_eq!(body(Lang::Id, Evening, 2, 5), "Besok 2 acara");
        assert_eq!(body(Lang::Id, Evening, 0, 0), "Besok belum ada acara");
        assert_eq!(body(Lang::Id, Evening, 0, 3), "Besok belum ada acara");
        assert_eq!(body(Lang::En, Evening, 2, 0), "2 events tomorrow");
        assert_eq!(body(Lang::En, Evening, 1, 0), "1 event tomorrow");
        assert_eq!(body(Lang::En, Evening, 0, 0), "Nothing planned tomorrow");
        // Night: "late", then the coming day's events.
        assert_eq!(body(Lang::Id, Night, 2, 0), "Sudah larut. Besok 2 acara");
        assert_eq!(
            body(Lang::Id, Night, 0, 0),
            "Sudah larut. Besok belum ada acara"
        );
        assert_eq!(body(Lang::En, Night, 2, 0), "It's late. 2 events tomorrow");
        assert_eq!(
            body(Lang::En, Night, 0, 0),
            "It's late. Nothing planned tomorrow"
        );
        // Never a zero part.
        for lang in [Lang::Id, Lang::En] {
            for p in ALL_PERIODS {
                for (e, r) in [(0, 0), (0, 2), (3, 0), (1, 1)] {
                    let s = body(lang, p, e, r);
                    assert!(!s.starts_with('0') && !s.contains(" 0 "), "{s:?}");
                }
            }
        }
        // The settings rows, as in the design.
        // The mascot has no name yet (the owner picks it): it is Vale's mascot.
        assert_eq!(
            text(Lang::Id, Key::RobotInNotchDesc),
            "Tampilkan maskot Vale di samping jam dan notice"
        );
        assert_eq!(
            [Lang::Id, Lang::En].map(|l| text(l, Key::SourceVale)),
            ["Vale", "Vale"]
        );
        assert_eq!(
            [Key::MotionFull, Key::MotionSaver, Key::MotionOff].map(|k| text(Lang::Id, k)),
            ["Penuh", "Hemat", "Mati"]
        );
    }

    #[test]
    fn the_labels_say_maskot() {
        assert_eq!(text(Lang::Id, Key::SecCat), "Maskot");
        assert_eq!(text(Lang::Id, Key::RobotInNotch), "Maskot di notch");
        assert_eq!(text(Lang::En, Key::MascotFalk), "Cockatiel");
        let both = |k: Key| [Lang::Id, Lang::En].map(|l| text(l, k));
        assert_eq!(both(Key::SecCat), ["Maskot", "Mascot"]);
        assert_eq!(both(Key::GroupCat), ["Maskot", "Mascot"]);
        assert_eq!(both(Key::RobotInNotch), ["Maskot di notch", "Mascot in the notch"]);
        assert_eq!(
            both(Key::RobotInNotchDesc),
            [
                "Tampilkan maskot Vale di samping jam dan notice",
                "Show Vale's mascot beside the clock and notices"
            ]
        );
        assert_eq!(both(Key::MascotLabel), ["Maskot", "Mascot"]);
        assert_eq!(both(Key::MascotCat), ["Kucing", "Cat"]);
        assert_eq!(both(Key::MascotFalk), ["Falk", "Cockatiel"]);
        assert_eq!(both(Key::MascotOctopus), ["Gurita", "Octopus"]);
        assert_eq!(both(Key::FalkColorLutino), ["Lutino", "Lutino"]);
        assert_eq!(both(Key::FalkColorGrey), ["Abu-abu", "Grey"]);
        assert_eq!(both(Key::FalkColorWhiteFace), ["Muka putih", "White face"]);
        assert_eq!(both(Key::OctopusColorCoral), ["Koral", "Coral"]);
        assert_eq!(both(Key::OctopusColorRed), ["Merah", "Red"]);
        assert_eq!(both(Key::OctopusColorPurple), ["Ungu", "Purple"]);
        assert_eq!(both(Key::OctopusColorBlue), ["Biru laut", "Sea blue"]);
        assert_eq!(both(Key::AccSailor), ["Topi pelaut", "Sailor cap"]);
        assert_eq!(both(Key::AccBowtie), ["Dasi kupu-kupu", "Bow tie"]);
        assert_eq!(text(Lang::Id, Key::AccHeadphone), "Headphone");
        assert_eq!(both(Key::SurfaceAuto), ["Ikuti maskot", "Match mascot"]);
        // The sticky notes' peek stays the cat's.
        assert_eq!(text(Lang::Id, Key::WidgetsPeek), "Kucing ngintip");
    }

    #[test]
    fn indonesian_strings_are_unchanged() {
        assert_eq!(text(Lang::Id, Key::ClaudeWaiting), "Claude butuh respon");
        assert_eq!(text(Lang::Id, Key::AddEvent), "Tambah acara");
        assert_eq!(text(Lang::En, Key::AddEvent), "Add event");
        assert_eq!(text(Lang::En, Key::ReconnectGoogle), "Reconnect Google");
    }

    #[test]
    fn current_language_defaults_to_indonesian_and_switches() {
        assert_eq!(lang(), Lang::Id);
        assert_eq!(tr(Key::BatteryLow), "Baterai lemah");
        set_lang(Lang::En);
        assert_eq!(lang(), Lang::En);
        assert_eq!(tr(Key::BatteryLow), "Low battery");
        set_lang(Lang::Id);
        assert_eq!(tr(Key::BatteryLow), "Baterai lemah");
    }

    #[test]
    fn codes_round_trip_and_unknown_is_indonesian() {
        for lang in [Lang::Id, Lang::En] {
            let json = serde_json::to_string(&lang).unwrap();
            assert_eq!(json, format!("\"{}\"", lang.code()));
            assert_eq!(serde_json::from_str::<Lang>(&json).unwrap(), lang);
        }
        assert_eq!(Lang::from_code(" EN "), Some(Lang::En));
        assert_eq!(Lang::from_code("fr"), None);
        assert_eq!(serde_json::from_str::<Lang>("\"fr\"").unwrap(), Lang::Id);
        assert!(serde_json::from_str::<Lang>("3").is_err());
    }

    #[test]
    fn dates_in_indonesian() {
        let l = Lang::Id;
        assert_eq!(month_title(l, date(2026, 10, 25)), "Oktober 2026");
        assert_eq!(day_title_long(l, date(2026, 10, 26)), "Senin, 26 Oktober");
        assert_eq!(week_title(l, date(2026, 10, 19)), "19 – 25 Okt 2026");
        assert_eq!(week_title(l, date(2026, 9, 28)), "28 Sep – 4 Okt 2026");
        assert_eq!(
            week_title(l, date(2025, 12, 29)),
            "29 Des 2025 – 4 Jan 2026"
        );
        assert_eq!(day_short(l, date(2026, 10, 19)), "Sen");
        assert_eq!(day_title_long(l, date(2026, 10, 25)), "Minggu, 25 Oktober");
    }

    #[test]
    fn dates_in_english() {
        let l = Lang::En;
        assert_eq!(month_title(l, date(2026, 10, 25)), "October 2026");
        assert_eq!(month_title(l, date(2026, 5, 1)), "May 2026");
        assert_eq!(day_title_long(l, date(2026, 10, 26)), "Monday, 26 October");
        assert_eq!(week_title(l, date(2026, 10, 19)), "19 – 25 Oct 2026");
        assert_eq!(week_title(l, date(2026, 9, 28)), "28 Sep – 4 Oct 2026");
        assert_eq!(
            week_title(l, date(2025, 12, 29)),
            "29 Dec 2025 – 4 Jan 2026"
        );
        assert_eq!(day_short(l, date(2026, 10, 21)), "Wed");
        assert_eq!(day_title_long(l, date(2026, 10, 25)), "Sunday, 25 October");
    }

    #[test]
    fn time_separator_follows_the_language() {
        let t = date(2026, 10, 25).and_time(NaiveTime::from_hms_opt(9, 5, 0).unwrap());
        assert_eq!(time_hm(Lang::Id, t), "09.05");
        assert_eq!(time_hm(Lang::En, t), "09:05");
    }

    #[test]
    fn formatted_strings_in_both_languages() {
        assert_eq!(
            focus_done_body(Lang::Id, 5),
            "Istirahat 5 menit · klik untuk mulai"
        );
        assert_eq!(
            focus_done_body(Lang::En, 15),
            "15-minute break · click to start"
        );
        assert_eq!(
            break_done_body(Lang::Id, 2, 4),
            "Fokus sesi 2 dari 4 · klik untuk mulai"
        );
        assert_eq!(
            break_done_body(Lang::En, 2, 4),
            "Focus session 2 of 4 · click to start"
        );
        assert_eq!(session_of(Lang::Id, 1, 4), "Sesi 1 dari 4");
        assert_eq!(session_of(Lang::En, 1, 4), "Session 1 of 4");
        assert_eq!(pomo_durations(Lang::Id, 25, 5, 15), "25 · 5 · 15 menit");
        assert_eq!(pomo_durations(Lang::En, 50, 10, 20), "50 · 10 · 20 min");
        assert_eq!(video_estimate_line(Lang::Id, 1920, 1080, 36), "1920×1080 · ± 36 MB/menit");
        assert_eq!(video_estimate_line(Lang::En, 1920, 1080, 36), "1920×1080 · ± 36 MB/min");
        assert_eq!(phase_minutes(Lang::Id, "Istirahat", 5), "Istirahat 5 menit");
        assert_eq!(phase_minutes(Lang::En, "Focus", 25), "Focus · 25 min");
        assert_eq!(more_count(Lang::Id, 3), "+3 lagi");
        assert_eq!(more_count(Lang::En, 3), "+3 more");
        assert_eq!(battery_body(Lang::Id, 18), "Sisa 18%. Colokin charger.");
        assert_eq!(battery_body(Lang::En, 18), "18% left. Plug in the charger.");
        assert_eq!(
            save_failed_because(Lang::Id, text(Lang::Id, Key::ReasonOffline)),
            "Gagal simpan: tidak ada koneksi"
        );
        assert_eq!(
            save_failed_because(Lang::En, &google_refused(Lang::En, "Not Found")),
            "Couldn't save: Google refused (Not Found)"
        );
        assert_eq!(
            config_copied_to(Lang::En, "config.json.bak"),
            "Copied to config.json.bak — check its contents"
        );
        let page = oauth_page(Lang::En, Key::OauthConnected);
        assert!(page.starts_with("<!doctype html><html lang=\"en\">"));
        assert!(page.contains("Connected. You can go back to the island."));
    }
}

#[cfg(test)]
mod copy_preview_tests {
    use super::*;
    use crate::clipboard::ClipKind;

    #[test]
    fn copy_preview_texts() {
        assert_eq!(
            clip_text_meta(Lang::Id, ClipKind::Text, 42),
            "Teks · 42 karakter"
        );
        assert_eq!(
            clip_text_meta(Lang::En, ClipKind::Text, 42),
            "Text · 42 characters"
        );
        assert_eq!(
            clip_text_meta(Lang::En, ClipKind::Code, 1),
            "Code · 1 character"
        );
        // A link or a colour is its kind's name alone.
        assert_eq!(clip_text_meta(Lang::Id, ClipKind::Link, 30), "Tautan");
        assert_eq!(clip_text_meta(Lang::En, ClipKind::Color, 7), "Color");
        assert_eq!(clip_preview_size(1280, 720), "1280 × 720");
        for (lang, title, files, chip) in [
            (Lang::Id, "Gambar disalin", "Berkas disalin", "Riwayat"),
            (Lang::En, "Image copied", "Files copied", "History"),
        ] {
            assert_eq!(text(lang, Key::ClipPreviewImage), title);
            assert_eq!(text(lang, Key::ClipPreviewFiles), files);
            assert_eq!(text(lang, Key::ClipPreviewChip), chip);
        }
        assert_eq!(text(Lang::Id, Key::ClipCopyNotice), "Pratinjau salinan");
        assert_eq!(text(Lang::En, Key::ClipCopyNotice), "Copy preview");
    }

    #[test]
    fn the_phone_link_strings_read_in_both_languages() {
        use crate::phone::DeviceKind;
        use crate::phone::net::Trust;
        assert_eq!(phone_valid_for(Lang::Id, 292), "berlaku 4:52");
        assert_eq!(phone_valid_for(Lang::En, 7), "valid 0:07");
        assert_eq!(
            phone_network(Lang::Id, "Rumah-5G", Trust::Private),
            "Rumah-5G · pribadi"
        );
        assert_eq!(
            phone_network(Lang::En, "Home", Trust::Private),
            "Home · private"
        );
        assert_eq!(
            phone_network(Lang::Id, "Kenzie_5G", Trust::Public),
            "Kenzie_5G · publik"
        );
        assert_eq!(
            phone_network(Lang::En, "Cafe", Trust::Public),
            "Cafe · public"
        );
        // One phone by its kind, several by their number.
        assert_eq!(phone_devices_chip(Lang::Id, &[]), "");
        assert_eq!(phone_devices_chip(Lang::Id, &[DeviceKind::IPhone]), "iPhone");
        assert_eq!(
            phone_devices_chip(Lang::Id, &[DeviceKind::IPhone, DeviceKind::Android]),
            "2 HP"
        );
        assert_eq!(
            phone_devices_chip(Lang::En, &[DeviceKind::IPhone, DeviceKind::Android]),
            "2 phones"
        );
        // Step 2's line once a phone joined.
        assert_eq!(
            phone_connected(Lang::Id, &[DeviceKind::Android]),
            "Tersambung: Android"
        );
        assert_eq!(
            phone_connected(Lang::Id, &[DeviceKind::Android, DeviceKind::IPhone]),
            "Tersambung: 2 HP"
        );
        assert_eq!(
            phone_connected(Lang::En, &[DeviceKind::IPhone]),
            "Connected: iPhone"
        );
        assert_eq!(
            phone_connected(Lang::En, &[DeviceKind::Other; 2]),
            "Connected: 2 phones"
        );
        // Sizes go through `format_size` from 1 MB.
        assert_eq!(phone_size(Lang::Id, 812 * 1024), "812 KB");
        assert_eq!(phone_size(Lang::Id, 5 * MIB), "5 MB");
        assert_eq!(phone_size(Lang::Id, 1900 * MIB), format_size(Lang::Id, 1900 * MIB));
        assert_eq!(
            phone_received_summary(Lang::Id, 3, 55 * MIB),
            r"3 berkas · 55 MB → Unduhan\Vale"
        );
        assert_eq!(
            phone_received_summary(Lang::En, 1, 2 * MIB),
            r"1 file · 2 MB → Downloads\Vale"
        );
        assert_eq!(text(Lang::Id, Key::ShelfMissing), "hilang");
        assert_eq!(
            text(Lang::Id, Key::PhoneStep2),
            "Kirim atau ambil berkas di HP"
        );
        assert_eq!(
            text(Lang::En, Key::PhoneStep2Sub),
            "Every shelf file shows there"
        );
        for key in [
            Key::ShelfPhone,
            Key::PhoneCancelAll,
            Key::PhoneFinish,
            Key::PhoneNewQr,
            Key::PhoneStep2,
            Key::PhoneStep2Sub,
            Key::PhoneNetPublic,
            Key::PhoneConnected,
            Key::PhonePublicHint,
        ] {
            assert_ne!(text(Lang::Id, key), text(Lang::En, key), "{key:?}");
        }
    }
}
