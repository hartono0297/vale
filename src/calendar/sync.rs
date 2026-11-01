//! The calendar sync thread: every Google call runs here, never on the UI
//! thread. It answers `SyncRequest`s with `Event::Calendar` snapshots (and,
//! for saves, `WM_APP_EDITOR_STATUS` to the editor window), keeps the local
//! cache, and re-fetches the last requested range every ten minutes.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use super::{CalStatus, CalendarSnapshot, EventDraft, api, range_for};
use crate::config::{self, Config};
use crate::google::client;
use crate::google::http::Http;
use crate::google::oauth::{self, AuthError};
use crate::i18n::{self, Key, Lang, text, tr};
use crate::model::{CalView, Event, Notice, NoticeKind};

/// Posted to the event editor after a save: `wparam` 1 = saved, 0 = failed with
/// `lparam` a `Box<String>` raw pointer (Indonesian message) that the receiver
/// frees; `lparam` is 0 on success. (WM_APP + 1..4 are taken by the island.)
pub const WM_APP_EDITOR_STATUS: u32 = WM_APP + 5;

/// Values table: re-fetch the last range every 10 minutes.
const REFRESH_EVERY: Duration = Duration::from_secs(10 * 60);
/// A repeat request for the range fetched successfully this recently is skipped.
const REPEAT_WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum SyncRequest {
    /// Fetch `[from, to)` and make it the range the timer re-fetches.
    Range {
        from: NaiveDate,
        to: NaiveDate,
    },
    /// Patch `event_id` (in `calendar_id`, default `primary`), or insert a new
    /// event in `primary` when `event_id` is `None`. `reply` is the editor's HWND.
    Save {
        event_id: Option<String>,
        calendar_id: Option<String>,
        draft: EventDraft,
        reply: isize,
    },
    /// Interactive Google login (opens the browser), then a fetch. Sent by
    /// `SyncHandle::connect`, which keeps a second one from queueing.
    Connect,
    /// config.json changed on disk (Google client ID or secret): use it from
    /// the next request on, without a restart.
    Config(Config),
    /// "Putuskan" in the settings window: forget the stored tokens. The
    /// calendar then asks to be connected again; the cached events stay.
    Disconnect,
    Shutdown,
}

/// The UI thread's end of the sync thread.
pub struct SyncHandle {
    requests: Sender<SyncRequest>,
    /// A `Connect` is queued or its browser login is running.
    login: Arc<AtomicBool>,
}

impl SyncHandle {
    /// False when the sync thread is gone.
    pub fn send(&self, req: SyncRequest) -> bool {
        self.requests.send(req).is_ok()
    }

    /// Starts the Google login unless one is already under way (the login
    /// waits up to minutes for the browser); false when ignored or the thread is gone.
    pub fn connect(&self) -> bool {
        if self.login.swap(true, Ordering::AcqRel) {
            return false;
        }
        if self.send(SyncRequest::Connect) {
            return true;
        }
        self.login.store(false, Ordering::Release);
        false
    }
}

/// Shown on the island when a save fails after its editor was closed.
pub fn save_failed_notice(message: String) -> Notice {
    Notice {
        key: "calendar-save-failed".into(),
        source: tr(Key::SourceCalendar).into(),
        title: tr(Key::SaveEventFailed).into(),
        body: message,
        kind: NoticeKind::App { app_id: None, toast: None },
    }
}

/// `%APPDATA%\Vale\calendar-cache.json`, next to the config file.
pub fn cache_path() -> PathBuf {
    config::path().with_file_name("calendar-cache.json")
}

/// A missing or malformed cache is `None`.
pub fn load_cache(path: &Path) -> Option<CalendarSnapshot> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok()
}

/// Pretty JSON to a sibling `.tmp` file, then renamed over the cache.
pub fn save_cache(path: &Path, snapshot: &CalendarSnapshot) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(snapshot).map_err(io::Error::other)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// The snapshot after a fetch of `[from, to)`: fresh events on success; on
/// failure the previous events and range stay, with the status the error means.
pub fn merge_fetch(
    prev: &CalendarSnapshot,
    result: Result<Vec<super::CalEvent>, AuthError>,
    from: NaiveDate,
    to: NaiveDate,
    now: DateTime<Local>,
) -> CalendarSnapshot {
    match result {
        Ok(events) => CalendarSnapshot {
            status: CalStatus::Ok,
            events,
            from,
            to,
            fetched_at: Some(now),
        },
        Err(e) => CalendarSnapshot {
            status: status_for(&e),
            ..prev.clone()
        },
    }
}

/// The status after a browser login that did not succeed. With a client ID
/// set, every failure (cancelled, timed out after five minutes, never
/// redirected, or the code exchange failed) is `NeedsLogin`, so the connect
/// button stays on screen; `Offline` would hide it. Events are kept either way.
fn login_failure_status(configured: bool) -> CalStatus {
    if configured {
        CalStatus::NeedsLogin
    } else {
        CalStatus::NotConfigured
    }
}

/// Only transport failures (and 5xx / rate limits) read as `Offline`; a
/// refusal from Google keeps the connect button on screen, with a notice
/// saying why (`google_notice`).
fn status_for(e: &AuthError) -> CalStatus {
    match e {
        AuthError::NotConfigured => CalStatus::NotConfigured,
        AuthError::NeedsLogin
        | AuthError::MissingScopes
        | AuthError::Google(_)
        | AuthError::ClientChanged => CalStatus::NeedsLogin,
        AuthError::Reconnect | AuthError::Unauthorized => CalStatus::Reconnect,
        AuthError::Network(_) => CalStatus::Offline,
    }
}

/// The island notice for an error the calendar status alone cannot explain:
/// unticked scopes, or a refusal from Google.
fn google_notice(e: &AuthError) -> Option<Notice> {
    let (title, body) = match e {
        AuthError::MissingScopes => (tr(Key::GoogleScopes), tr(Key::GoogleScopesBody).into()),
        AuthError::Google(message) => (tr(Key::GoogleRefused), message.clone()),
        AuthError::ClientChanged => (
            tr(Key::GoogleClientChanged),
            tr(Key::GoogleClientChangedBody).into(),
        ),
        _ => return None,
    };
    Some(Notice {
        key: "calendar-google".into(),
        source: tr(Key::SourceCalendar).into(),
        title: title.into(),
        body,
        kind: NoticeKind::App { app_id: None, toast: None },
    })
}

/// With a client set (`own`: the user's own or Vale's built-in one),
/// `NotConfigured` means Google rejected that client: a refusal saying what
/// to do, which keeps the connect button (NeedsLogin), not "not set up".
fn rejected(e: AuthError, own: Option<bool>) -> AuthError {
    match (e, own) {
        (AuthError::NotConfigured, Some(true)) => AuthError::Google(tr(Key::ClientRejected).into()),
        (AuthError::NotConfigured, Some(false)) => {
            AuthError::Google(tr(Key::BuiltInRejected).into())
        }
        (e, _) => e,
    }
}

/// The notice after a browser login that failed (`own` as in `rejected`).
fn login_notice(e: &AuthError, own: Option<bool>) -> Option<Notice> {
    google_notice(&rejected(e.clone(), own))
}

/// Whose client the config gives: `Some(true)` the user's own,
/// `Some(false)` Vale's built-in one, `None` none.
fn client_owner(cfg: &Config) -> Option<bool> {
    client::for_config(cfg).map(|c| c.own)
}

/// Runs `call` with `token`; on a 401 gets a fresh token from `refresh` and
/// tries once more. A second 401 means the login is no good (`Reconnect`).
fn retry_unauthorized<T>(
    token: String,
    call: impl Fn(&str) -> Result<T, AuthError>,
    refresh: impl FnOnce() -> Result<String, AuthError>,
) -> Result<T, AuthError> {
    match call(&token) {
        Err(AuthError::Unauthorized) => call(&refresh()?).map_err(|e| match e {
            AuthError::Unauthorized => AuthError::Reconnect,
            other => other,
        }),
        other => other,
    }
}

/// The editor's error line. Network messages that start with "HTTP " came
/// from Google (a 5xx); any other network error means the request never got
/// through.
pub fn save_error_message(lang: Lang, e: &AuthError) -> String {
    let reason = |key| text(lang, key).to_string();
    let reason = match e {
        AuthError::NotConfigured => reason(Key::ReasonNotConfigured),
        AuthError::NeedsLogin | AuthError::ClientChanged => reason(Key::ReasonNeedsLogin),
        AuthError::Reconnect | AuthError::Unauthorized => reason(Key::ReasonReconnect),
        AuthError::MissingScopes => reason(Key::ReasonScopes),
        AuthError::Google(m) => i18n::google_refused(lang, m),
        AuthError::Network(m) if m.starts_with("HTTP ") => i18n::google_refused(lang, m),
        AuthError::Network(_) => reason(Key::ReasonOffline),
    };
    i18n::save_failed_because(lang, &reason)
}

/// Starts the sync thread. It sends the cached snapshot right away (status
/// `Offline` until a fetch succeeds); the caller asks for the first range.
pub fn spawn_sync(tx: Sender<Event>, wake: impl Fn() + Send + 'static, cfg: Config) -> SyncHandle {
    spawn_with(tx, wake, cfg, cache_path()).0
}

fn spawn_with(
    tx: Sender<Event>,
    wake: impl Fn() + Send + 'static,
    cfg: Config,
    cache: PathBuf,
) -> (SyncHandle, JoinHandle<()>) {
    let (req_tx, req_rx) = mpsc::channel();
    let login = Arc::new(AtomicBool::new(false));
    let mut worker = Worker::new(tx, Box::new(wake), cfg, cache);
    worker.login = Arc::clone(&login);
    let handle = std::thread::spawn(move || worker.run(req_rx));
    let sync = SyncHandle {
        requests: req_tx,
        login,
    };
    (sync, handle)
}

struct Worker {
    tx: Sender<Event>,
    wake: Box<dyn Fn() + Send>,
    cfg: Config,
    cache: PathBuf,
    /// Shared with `SyncHandle`; cleared once a login attempt ends.
    login: Arc<AtomicBool>,
    /// Created on first use; WinRT setup failures are retried next time.
    http: Option<Http>,
    snapshot: CalendarSnapshot,
    /// The last requested range, re-fetched by the timer.
    range: Option<(NaiveDate, NaiveDate)>,
    last_ok: Option<((NaiveDate, NaiveDate), Instant)>,
    next_refresh: Option<Instant>,
    /// Title and body of every fetch-error notice shown this run, so the
    /// ten-minute refresh does not repeat one.
    noticed: HashSet<(String, String)>,
    /// Whether Google tokens are stored (replaced in tests, which must not
    /// read the real token file).
    has_tokens: fn() -> bool,
}

impl Worker {
    fn new(tx: Sender<Event>, wake: Box<dyn Fn() + Send>, cfg: Config, cache: PathBuf) -> Self {
        let status = if configured(&cfg) {
            CalStatus::Offline
        } else {
            CalStatus::NotConfigured
        };
        let snapshot = load_cache(&cache).map_or_else(
            || CalendarSnapshot::empty(status, Local::now().date_naive()),
            |cached| CalendarSnapshot { status, ..cached },
        );
        Worker {
            tx,
            wake,
            cfg,
            cache,
            login: Arc::new(AtomicBool::new(false)),
            http: None,
            snapshot,
            range: None,
            last_ok: None,
            next_refresh: None,
            noticed: HashSet::new(),
            has_tokens: || oauth::load_tokens().is_some(),
        }
    }

    fn run(mut self, rx: Receiver<SyncRequest>) {
        self.emit();
        loop {
            let first = match self.next_refresh {
                Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                    Ok(req) => req,
                    Err(RecvTimeoutError::Timeout) => {
                        self.refresh(true);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match rx.recv() {
                    Ok(req) => req,
                    Err(_) => return,
                },
            };
            // Everything queued meanwhile is handled as one batch, so a burst of
            // navigation fetches only the latest range.
            let batch: Vec<SyncRequest> = std::iter::once(first).chain(rx.try_iter()).collect();
            if !self.handle(batch) {
                return;
            }
        }
    }

    /// False on `Shutdown`.
    fn handle(&mut self, batch: Vec<SyncRequest>) -> bool {
        // `Some(force)` when the batch ends with a fetch.
        let mut fetch: Option<bool> = None;
        for req in batch {
            match req {
                SyncRequest::Range { from, to } => {
                    self.range = Some((from, to));
                    fetch = Some(fetch.unwrap_or(false));
                }
                SyncRequest::Save {
                    event_id,
                    calendar_id,
                    draft,
                    reply,
                } => {
                    let result = self.save(event_id.as_deref(), calendar_id.as_deref(), &draft);
                    let result = result.map_err(|e| save_error_message(i18n::lang(), &e));
                    if let Some(message) = post_status(reply, result) {
                        // The editor was closed while saving: tell the island instead.
                        self.send(Event::Notice(save_failed_notice(message)));
                    }
                    fetch = Some(true);
                }
                SyncRequest::Connect => {
                    if self.connect() {
                        fetch = Some(true);
                    }
                }
                SyncRequest::Config(cfg) => self.set_config(cfg),
                SyncRequest::Disconnect => self.disconnect(),
                SyncRequest::Shutdown => return false,
            }
        }
        if let Some(force) = fetch {
            self.refresh(force);
        }
        true
    }

    /// Fetches the last requested range (this month before any request).
    fn refresh(&mut self, force: bool) {
        if !configured(&self.cfg) {
            self.set_status(CalStatus::NotConfigured);
            return;
        }
        let range = self
            .range
            .unwrap_or_else(|| range_for(CalView::Month, Local::now().date_naive()));
        let recent = self
            .last_ok
            .is_some_and(|(r, at)| r == range && at.elapsed() < REPEAT_WINDOW);
        if recent && !force {
            return;
        }
        let own = client_owner(&self.cfg);
        let result = self
            .with_token(|http, token| api::fetch_range(http, token, range.0, range.1))
            .map_err(|e| rejected(e, own));
        let ok = result.is_ok();
        if let Some(notice) = result.as_ref().err().and_then(google_notice) {
            self.notify_once(notice);
        }
        self.snapshot = merge_fetch(&self.snapshot, result, range.0, range.1, Local::now());
        if ok {
            // A failed write only costs the offline view after a restart.
            let _ = save_cache(&self.cache, &self.snapshot);
            self.last_ok = Some((range, Instant::now()));
        }
        self.next_refresh = Some(Instant::now() + REFRESH_EVERY);
        self.emit();
    }

    fn save(
        &mut self,
        event_id: Option<&str>,
        calendar_id: Option<&str>,
        draft: &EventDraft,
    ) -> Result<(), AuthError> {
        if !configured(&self.cfg) {
            return Err(AuthError::NotConfigured);
        }
        self.with_token(|http, token| match event_id {
            Some(id) => api::patch_event(http, token, calendar_id.unwrap_or("primary"), id, draft),
            None => api::insert_event(http, token, draft),
        })
    }

    /// Runs `call` with a valid access token; a 401 gets one forced refresh
    /// and retry.
    fn with_token<T>(
        &mut self,
        call: impl Fn(&Http, &str) -> Result<T, AuthError>,
    ) -> Result<T, AuthError> {
        let http = ensure_http(&mut self.http)?;
        let cfg = &self.cfg;
        let token = oauth::access_token(http, cfg)?;
        retry_unauthorized(
            token,
            |token| call(http, token),
            || oauth::force_refresh(http, cfg),
        )
    }

    /// Posts `notice` unless one with the same title and body was posted this run.
    fn notify_once(&mut self, notice: Notice) {
        if self
            .noticed
            .insert((notice.title.clone(), notice.body.clone()))
        {
            self.send(Event::Notice(notice));
        }
    }

    /// Runs the browser login. True when it succeeded and a fetch should follow;
    /// on failure the snapshot shows why, and a notice too when Google refused
    /// or a permission was left unticked.
    fn connect(&mut self) -> bool {
        let result = match client::for_config(&self.cfg) {
            Some(client) => {
                ensure_http(&mut self.http).and_then(|http| oauth::login(http, &client))
            }
            None => Err(AuthError::NotConfigured),
        };
        // The attempt is over, before the island hears of it: a new one may start.
        self.login.store(false, Ordering::Release);
        match result {
            Ok(_) => {
                // A fresh login: a refusal that is still there after it is news.
                self.noticed.clear();
                true
            }
            // Whatever went wrong, the next step is another click on the
            // connect button; a notice says what to do differently when
            // there is something (a cancel or timeout needs no words).
            Err(e) => {
                let configured = configured(&self.cfg);
                if let Some(notice) = login_notice(&e, client_owner(&self.cfg)) {
                    // Shown on every failed attempt: each one was a click.
                    self.send(Event::Notice(notice));
                }
                self.set_status(login_failure_status(configured));
                false
            }
        }
    }

    /// A client ID added (or removed) by hand takes effect at once: the
    /// calendar stops (or starts) showing "not configured", without calling Google.
    fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
        match (configured(&self.cfg), self.snapshot.status) {
            // Set up from the settings window or by hand: say at once whether
            // a login is still needed, rather than "offline" until a fetch.
            (true, CalStatus::NotConfigured) => {
                let status = if (self.has_tokens)() {
                    CalStatus::Offline
                } else {
                    CalStatus::NeedsLogin
                };
                self.set_status(status);
            }
            (false, status) if status != CalStatus::NotConfigured => {
                self.set_status(CalStatus::NotConfigured)
            }
            _ => {}
        }
    }

    fn disconnect(&mut self) {
        oauth::clear_tokens();
        // Nothing to refresh until the next login.
        self.next_refresh = None;
        self.last_ok = None;
        if configured(&self.cfg) {
            self.set_status(CalStatus::NeedsLogin);
        }
    }

    fn set_status(&mut self, status: CalStatus) {
        self.snapshot.status = status;
        self.emit();
    }

    fn emit(&self) {
        self.send(Event::Calendar(self.snapshot.clone()));
    }

    fn send(&self, event: Event) {
        if self.tx.send(event).is_ok() {
            (self.wake)();
        }
    }
}

/// A client to log in with: the user's own or the built-in one.
fn configured(cfg: &Config) -> bool {
    client::for_config(cfg).is_some()
}

fn ensure_http(slot: &mut Option<Http>) -> Result<&Http, AuthError> {
    if slot.is_none() {
        *slot = Some(Http::new().map_err(|e| AuthError::Network(e.message()))?);
    }
    Ok(slot.as_ref().expect("just set"))
}

/// Tells the editor `reply` how the save went. Returns a failure's message
/// when no editor received it (closed meanwhile, or `reply` is 0).
pub fn post_status(reply: isize, result: Result<(), String>) -> Option<String> {
    let undelivered = result.as_ref().err().cloned();
    if reply == 0 {
        return undelivered;
    }
    let (wparam, lparam) = match result {
        Ok(()) => (1, 0),
        Err(message) => (0, Box::into_raw(Box::new(message)) as isize),
    };
    let hwnd = HWND(reply as *mut core::ffi::c_void);
    let posted = unsafe {
        PostMessageW(
            Some(hwnd),
            WM_APP_EDITOR_STATUS,
            WPARAM(wparam),
            LPARAM(lparam),
        )
    };
    if posted.is_err() {
        if lparam != 0 {
            // The editor is gone; nobody else will free the message.
            drop(unsafe { Box::from_raw(lparam as *mut String) });
        }
        return undelivered;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{CalEvent, EventTime};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// A fresh directory under the system temp dir; never `%APPDATA%`.
    fn temp_dir() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vale-cal-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> CalendarSnapshot {
        CalendarSnapshot {
            status: CalStatus::Ok,
            events: vec![
                CalEvent {
                    id: "a".into(),
                    calendar_id: "primary".into(),
                    title: "Libur".into(),
                    start: EventTime::AllDay(date(2026, 10, 26)),
                    end: EventTime::AllDay(date(2026, 10, 29)),
                    description: String::new(),
                    color: Some(0x4F8CFF),
                    editable: false,
                },
                CalEvent {
                    id: "b".into(),
                    calendar_id: "team".into(),
                    title: "Rapat \"mingguan\"".into(),
                    start: EventTime::At(
                        DateTime::parse_from_rfc3339("2026-10-25T10:00:00-07:00")
                            .unwrap()
                            .with_timezone(&Local),
                    ),
                    end: EventTime::At(
                        DateTime::parse_from_rfc3339("2026-10-25T11:00:00-07:00")
                            .unwrap()
                            .with_timezone(&Local),
                    ),
                    description: "baris 1\nbaris 2".into(),
                    color: None,
                    editable: true,
                },
            ],
            from: date(2026, 9, 28),
            to: date(2026, 11, 9),
            fetched_at: Some(Local::now()),
        }
    }

    #[test]
    fn cache_round_trip() {
        let dir = temp_dir();
        let path = dir.join("calendar-cache.json");
        assert_eq!(load_cache(&path), None, "missing file");
        let snap = sample();
        save_cache(&path, &snap).unwrap();
        let loaded = load_cache(&path);
        let leftover_tmp = dir.join("calendar-cache.json.tmp").exists();
        std::fs::write(&path, "{ not json").unwrap();
        let corrupt = load_cache(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(loaded, Some(snap));
        assert!(!leftover_tmp);
        assert_eq!(corrupt, None);
    }

    #[test]
    fn fetch_error_keeps_cached_events_offline() {
        let prev = sample();
        let now = Local::now();
        let merged = merge_fetch(
            &prev,
            Err(AuthError::Network("no route".into())),
            date(2026, 10, 19),
            date(2026, 10, 26),
            now,
        );
        assert_eq!(merged.status, CalStatus::Offline);
        assert_eq!(merged.events, prev.events);
        assert_eq!((merged.from, merged.to), (prev.from, prev.to));
        assert_eq!(merged.fetched_at, prev.fetched_at);

        for (err, status) in [
            (AuthError::Reconnect, CalStatus::Reconnect),
            (AuthError::Unauthorized, CalStatus::Reconnect),
            (AuthError::NeedsLogin, CalStatus::NeedsLogin),
            (AuthError::MissingScopes, CalStatus::NeedsLogin),
            (AuthError::Google("Forbidden".into()), CalStatus::NeedsLogin),
            (AuthError::NotConfigured, CalStatus::NotConfigured),
        ] {
            let merged = merge_fetch(&prev, Err(err), prev.from, prev.to, now);
            assert_eq!(merged.status, status);
            assert_eq!(merged.events, prev.events);
        }
    }

    #[test]
    fn failed_login_keeps_the_connect_button_and_the_events() {
        // A timed-out or unredirected login is a network error; it must not
        // read as Offline, which hides the connect button.
        assert_eq!(login_failure_status(true), CalStatus::NeedsLogin);
        assert_eq!(login_failure_status(false), CalStatus::NotConfigured);

        let dir = temp_dir();
        let (mut w, rx, _) = worker(dir.join("calendar-cache.json"));
        w.snapshot = sample();
        w.set_status(login_failure_status(true));
        let shown = rx.try_iter().last();
        let Some(Event::Calendar(shown)) = shown else {
            panic!("no snapshot sent: {shown:?}");
        };
        assert_eq!(shown.status, CalStatus::NeedsLogin);
        assert_eq!(shown.events, sample().events, "cached events stay");
        assert!(!dir.exists(), "nothing written");
    }

    #[test]
    fn fetch_success_replaces_events_and_range() {
        let prev = sample();
        let now = Local::now();
        let merged = merge_fetch(
            &prev,
            Ok(vec![]),
            date(2026, 10, 19),
            date(2026, 10, 26),
            now,
        );
        assert_eq!(
            merged,
            CalendarSnapshot {
                status: CalStatus::Ok,
                events: vec![],
                from: date(2026, 10, 19),
                to: date(2026, 10, 26),
                fetched_at: Some(now),
            }
        );
    }

    #[test]
    fn save_errors_are_indonesian() {
        assert_eq!(
            save_error_message(
                Lang::Id,
                &AuthError::Network("The server name cannot be resolved".into())
            ),
            "Gagal simpan: tidak ada koneksi"
        );
        assert_eq!(
            save_error_message(Lang::Id, &AuthError::Network("HTTP 403: Forbidden".into())),
            "Gagal simpan: Google menolak (HTTP 403: Forbidden)"
        );
        assert_eq!(
            save_error_message(Lang::Id, &AuthError::Reconnect),
            "Gagal simpan: hubungkan ulang Google"
        );
        assert_eq!(
            save_error_message(Lang::Id, &AuthError::Google("Not Found".into())),
            "Gagal simpan: Google menolak (Not Found)"
        );
        assert_eq!(
            save_error_message(Lang::Id, &AuthError::MissingScopes),
            "Gagal simpan: izin Google belum lengkap"
        );
    }

    #[test]
    fn save_errors_in_english() {
        assert_eq!(
            save_error_message(Lang::En, &AuthError::Reconnect),
            "Couldn't save: reconnect Google"
        );
        assert_eq!(
            save_error_message(Lang::En, &AuthError::Network("timeout".into())),
            "Couldn't save: no connection"
        );
    }

    #[test]
    fn google_refusals_get_a_notice() {
        let n = google_notice(&AuthError::MissingScopes).unwrap();
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Izin Google belum lengkap", "Centang kedua izin saat login")
        );
        let n = google_notice(&AuthError::Google(tr(Key::ApiDisabled).into())).unwrap();
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            (
                "Google menolak",
                "Google Calendar API belum diaktifkan — lihat README"
            )
        );
        for quiet in [
            AuthError::Network("timeout".into()),
            AuthError::NeedsLogin,
            AuthError::Reconnect,
            AuthError::NotConfigured,
        ] {
            assert!(google_notice(&quiet).is_none(), "{quiet:?}");
        }
        // NotConfigured with a client set means Google rejected that client.
        let n = login_notice(&AuthError::NotConfigured, Some(true)).unwrap();
        assert_eq!(n.title, "Google menolak");
        assert!(login_notice(&AuthError::NotConfigured, None).is_none());
        assert!(
            login_notice(&AuthError::NeedsLogin, Some(true)).is_none(),
            "cancelled"
        );
        assert_eq!(
            login_notice(&AuthError::MissingScopes, Some(true))
                .unwrap()
                .title,
            "Izin Google belum lengkap"
        );
    }

    #[test]
    fn a_rejected_client_is_not_called_unconfigured() {
        // The user's own client: they can fix it in Settings.
        assert_eq!(
            rejected(AuthError::NotConfigured, Some(true)),
            AuthError::Google(tr(Key::ClientRejected).into())
        );
        // Vale's own: nothing to fix in Settings; say so, and keep the
        // connect button (NeedsLogin, not NotConfigured).
        let built_in = rejected(AuthError::NotConfigured, Some(false));
        assert_eq!(built_in, AuthError::Google(tr(Key::BuiltInRejected).into()));
        assert_eq!(status_for(&built_in), CalStatus::NeedsLogin);
        // No client at all, or another error: as it was.
        assert_eq!(
            rejected(AuthError::NotConfigured, None),
            AuthError::NotConfigured
        );
        assert_eq!(
            rejected(AuthError::NeedsLogin, Some(false)),
            AuthError::NeedsLogin
        );
    }

    #[test]
    fn a_401_refreshes_once_and_retries() {
        use std::cell::RefCell;
        // Accepted after the refresh.
        let calls = RefCell::new(Vec::new());
        let call = |t: &str| {
            calls.borrow_mut().push(t.to_string());
            if t == "fresh" {
                Ok(7)
            } else {
                Err(AuthError::Unauthorized)
            }
        };
        assert_eq!(
            retry_unauthorized("old".into(), call, || Ok("fresh".into())),
            Ok(7)
        );
        assert_eq!(*calls.borrow(), ["old", "fresh"]);

        // Refused again: the login is no good.
        let calls = RefCell::new(0);
        let refused = |_: &str| -> Result<(), AuthError> {
            *calls.borrow_mut() += 1;
            Err(AuthError::Unauthorized)
        };
        assert_eq!(
            retry_unauthorized("old".into(), refused, || Ok("fresh".into())),
            Err(AuthError::Reconnect)
        );
        assert_eq!(*calls.borrow(), 2, "one retry only");

        // The refresh itself failed: its error, no second call.
        let calls = RefCell::new(0);
        let refused = |_: &str| -> Result<(), AuthError> {
            *calls.borrow_mut() += 1;
            Err(AuthError::Unauthorized)
        };
        assert_eq!(
            retry_unauthorized("old".into(), refused, || Err(AuthError::Network(
                "x".into()
            ))),
            Err(AuthError::Network("x".into()))
        );
        assert_eq!(*calls.borrow(), 1);

        // Anything but a 401 is not retried.
        let refreshed = RefCell::new(false);
        assert_eq!(
            retry_unauthorized(
                "t".into(),
                |_| -> Result<(), AuthError> { Err(AuthError::Google("Forbidden".into())) },
                || {
                    *refreshed.borrow_mut() = true;
                    Ok("fresh".into())
                }
            ),
            Err(AuthError::Google("Forbidden".into()))
        );
        assert!(!*refreshed.borrow());
    }

    #[test]
    fn each_distinct_refusal_is_told_once_per_run() {
        let dir = temp_dir();
        let (mut w, rx, _) = worker(dir.join("calendar-cache.json"));
        let bodies = |rx: &Receiver<Event>| -> Vec<String> {
            rx.try_iter()
                .map(|e| match e {
                    Event::Notice(n) => n.body,
                    other => panic!("unexpected {other:?}"),
                })
                .collect()
        };
        let disabled = google_notice(&AuthError::Google(tr(Key::ApiDisabled).into())).unwrap();
        let other = google_notice(&AuthError::Google("Forbidden".into())).unwrap();
        w.notify_once(disabled.clone());
        w.notify_once(disabled.clone());
        w.notify_once(other);
        w.notify_once(disabled);
        assert_eq!(bodies(&rx), [tr(Key::ApiDisabled), "Forbidden"]);
        assert!(!dir.exists(), "nothing written");
    }

    fn worker(cache: PathBuf) -> (Worker, Receiver<Event>, Arc<AtomicUsize>) {
        let (tx, rx) = mpsc::channel();
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        let wake = Box::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        });
        // Empty client id: the worker must answer without calling Google.
        let mut worker = Worker::new(tx, wake, Config::default(), cache);
        // Never the real token file.
        worker.has_tokens = || false;
        (worker, rx, wakes)
    }

    fn statuses(rx: &Receiver<Event>) -> Vec<CalStatus> {
        rx.try_iter()
            .map(|e| match e {
                Event::Calendar(s) => s.status,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    #[test]
    fn queued_ranges_coalesce_to_the_latest() {
        let dir = temp_dir();
        let (mut w, rx, wakes) = worker(dir.join("calendar-cache.json"));
        let range = |d| SyncRequest::Range {
            from: date(2026, 10, d),
            to: date(2026, 10, d + 1),
        };
        assert!(w.handle(vec![range(1), range(2), range(3)]));
        assert_eq!(w.range, Some((date(2026, 10, 3), date(2026, 10, 4))));
        assert_eq!(statuses(&rx), [CalStatus::NotConfigured], "one answer");
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        assert!(!w.handle(vec![range(5), SyncRequest::Shutdown]));
        assert!(!dir.exists(), "nothing written while not configured");
    }

    #[test]
    fn not_configured_thread_answers_without_google() {
        let dir = temp_dir();
        let cache = dir.join("calendar-cache.json");
        save_cache(&cache, &sample()).unwrap();
        let before = std::fs::read(&cache).unwrap();

        let (tx, rx) = mpsc::channel();
        let (req, handle) = spawn_with(tx, || {}, Config::default(), cache.clone());
        let recv = || rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let next = || match recv() {
            Event::Calendar(s) => s,
            other => panic!("unexpected {other:?}"),
        };
        let first = next();
        assert_eq!(first.status, CalStatus::NotConfigured);
        assert_eq!(first.events, sample().events, "cache loaded at start");

        assert!(req.send(SyncRequest::Range {
            from: date(2026, 10, 19),
            to: date(2026, 10, 26),
        }));
        assert_eq!(next().status, CalStatus::NotConfigured);
        assert!(req.connect());
        assert_eq!(next().status, CalStatus::NotConfigured);
        // The attempt ended, so another one may start.
        assert!(!req.login.load(Ordering::Acquire));
        assert!(req.send(SyncRequest::Save {
            event_id: None,
            calendar_id: None,
            draft: crate::calendar::draft_from(&sample().events[1]),
            reply: 0,
        }));
        // Nobody waits for this save, so its failure goes to the island.
        match recv() {
            Event::Notice(n) => {
                assert_eq!(n.title, "Gagal simpan acara");
                assert_eq!(n.body, "Gagal simpan: Google belum diatur");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(next().status, CalStatus::NotConfigured);
        assert!(req.send(SyncRequest::Shutdown));
        handle.join().unwrap();
        assert!(!req.send(SyncRequest::Shutdown), "thread gone");
        assert!(!req.connect());
        assert!(!req.login.load(Ordering::Acquire));
        let after = std::fs::read(&cache).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(before, after, "cache untouched");
    }

    #[test]
    fn second_connect_is_ignored_while_one_runs() {
        let (requests, rx) = mpsc::channel();
        let sync = SyncHandle {
            requests,
            login: Arc::new(AtomicBool::new(false)),
        };
        assert!(sync.connect());
        assert!(!sync.connect(), "login already under way");
        assert_eq!(rx.try_iter().count(), 1, "only one Connect queued");
        // The worker clears the flag when the attempt ends.
        sync.login.store(false, Ordering::Release);
        assert!(sync.connect());
    }

    #[test]
    fn status_without_a_receiver_returns_the_failure() {
        assert_eq!(post_status(0, Ok(())), None);
        assert_eq!(post_status(0, Err("x".into())), Some("x".into()));
        // A handle that is no window: the post fails and the message comes back.
        let gone = 0x7FFF_FFF0;
        assert_eq!(post_status(gone, Err("y".into())), Some("y".into()));
        assert_eq!(post_status(gone, Ok(())), None);
        let n = save_failed_notice("Gagal simpan: tidak ada koneksi".into());
        assert_eq!(n.source, "Kalender");
        assert_eq!(n.kind, NoticeKind::App { app_id: None, toast: None });
    }

    #[test]
    fn a_new_client_id_ends_not_configured_without_a_restart() {
        let dir = temp_dir();
        let (mut w, rx, _) = worker(dir.join("calendar-cache.json"));
        let with_id = Config {
            google_client_id: "id.apps.googleusercontent.com".into(),
            google_client_secret: "secret".into(),
            ..Config::default()
        };
        // Same batch order as the island sends it: the config, then the login.
        w.has_tokens = || true;
        assert!(w.handle(vec![SyncRequest::Config(with_id.clone())]));
        assert_eq!(w.cfg, with_id);
        assert_eq!(
            statuses(&rx),
            [CalStatus::Offline],
            "no longer NotConfigured"
        );
        // A repeat changes nothing and says nothing.
        assert!(w.handle(vec![SyncRequest::Config(with_id.clone())]));
        assert!(statuses(&rx).is_empty());
        // The ID removed again: back to NotConfigured.
        assert!(w.handle(vec![SyncRequest::Config(Config::default())]));
        assert_eq!(statuses(&rx), [CalStatus::NotConfigured]);
        // Set up with no login stored yet: it asks to connect.
        w.has_tokens = || false;
        assert!(w.handle(vec![SyncRequest::Config(with_id)]));
        assert_eq!(statuses(&rx), [CalStatus::NeedsLogin]);
        assert!(!dir.exists(), "nothing written");
    }

    #[test]
    fn configured_worker_starts_offline_from_cache() {
        let dir = temp_dir();
        let cache = dir.join("calendar-cache.json");
        save_cache(&cache, &sample()).unwrap();
        let (tx, _rx) = mpsc::channel();
        let cfg = Config {
            google_client_id: "id.apps.googleusercontent.com".into(),
            ..Config::default()
        };
        let w = Worker::new(tx, Box::new(|| {}), cfg, cache);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(w.snapshot.status, CalStatus::Offline);
        assert_eq!(w.snapshot.events, sample().events);
    }
}
