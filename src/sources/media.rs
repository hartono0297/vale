use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{IUnknown, Interface, Result};

use crate::lyrics::timeline_moved;
use crate::media_log::{self, Click, Outcome, StatusSeen};
use crate::model::{Event, MediaCommand, MediaInfo, Timeline};

/// Thumbnails larger than this are dropped rather than held in memory.
const MAX_ART_BYTES: u64 = 2 * 1024 * 1024;

/// AppUserModelIds (matched as a case-insensitive substring) of players that
/// only play music. Browsers are not here: one browser id covers every tab,
/// a video tab included.
const KNOWN_PLAYERS: &[&str] = &[
    "spotify",
    "applemusic",
    "itunes",
    "tidal",
    "deezer",
    "amazon music",
    "amazonmusic",
    "youtube-music",
    "youtubemusic",
    "th-ch",
    "soundcloud",
    "pandora",
    "foobar2000",
    "musicbee",
    "aimp",
    "winamp",
    "zunemusic",
    "dopamine",
    "plexamp",
];

/// How long the shown session still ranks as playing after it stops (a short
/// pause or an open between two tracks), so another playing session does not
/// take the island for a moment and hand it back.
const HOLD: Duration = Duration::from_secs(2);

/// A tracked session, as far as ranking cares.
#[derive(Clone, Debug, PartialEq)]
struct Candidate {
    key: u64,
    app: String,
    /// `None` for closed or stopped, else whether it ranks as playing
    /// (`rank_playing`, or held by `HOLD`).
    live: Option<bool>,
    music: bool,
}

/// Whether a known music player owns this AppUserModelId.
fn known_player(app: &str) -> bool {
    let app = app.to_lowercase();
    KNOWN_PLAYERS.iter().any(|known| app.contains(known))
}

/// Music rather than a video or a social tab: a known player, or both artist
/// and album filled in. An artist alone is not enough (a video tab often has
/// a channel name). The session's declared `PlaybackType` is not used:
/// Chromium, Firefox and every Electron or WebView2 host report Music for
/// every tab, a video included.
fn is_music(app: &str, artist: &str, album: &str) -> bool {
    known_player(app) || (!artist.trim().is_empty() && !album.trim().is_empty())
}

/// The candidate to show, by index. Closed and stopped sessions are never
/// chosen. Of the rest the best by (playing, music, Windows' current session,
/// the one shown before); a full tie takes the first in list order.
fn choose(cands: &[Candidate], current: Option<u64>, shown: Option<u64>) -> Option<usize> {
    let mut best: Option<(usize, (bool, bool, bool, bool))> = None;
    for (i, c) in cands.iter().enumerate() {
        let Some(playing) = c.live else {
            continue;
        };
        let score = (
            playing,
            c.music,
            Some(c.key) == current,
            Some(c.key) == shown,
        );
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

/// What a WinRT event handler reports to the watcher thread. Handlers run on
/// WinRT threads, so they only send one of these and return. The key names
/// the tracked session the event came from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Signal {
    /// The manager's current session, or its set of sessions, changed.
    Session,
    /// A session's title, artist or thumbnail changed.
    Properties(u64),
    /// A session's play/pause state changed.
    Playback(u64),
    /// The shown session's position, length or seek state changed.
    Timeline(u64),
}

/// The work implied by a burst of signals, coalesced so that a flurry of
/// events costs one rebuild.
#[derive(Clone, Debug, Default, PartialEq)]
struct Refresh {
    /// List the manager's sessions again.
    session: bool,
    /// Sessions whose title, artist or album must be re-read.
    properties: Vec<u64>,
    /// Re-read the statuses and choose the session to show again.
    rank: bool,
    /// Re-read the timeline (cheap: no art).
    timeline: bool,
}

impl Refresh {
    fn add(&mut self, signal: Signal) {
        match signal {
            Signal::Session => {
                self.session = true;
                self.rank = true;
                self.timeline = true;
            }
            // A new track starts a new timeline, whichever event comes first.
            Signal::Properties(key) => {
                if !self.properties.contains(&key) {
                    self.properties.push(key);
                }
                self.rank = true;
                self.timeline = true;
            }
            // Any session's play/pause can change which one is shown; it may
            // come with or without a timeline update.
            Signal::Playback(_) => {
                self.rank = true;
                self.timeline = true;
            }
            Signal::Timeline(_) => self.timeline = true,
        }
    }
}

/// Remembers the last value sent so that repeated events with identical
/// content are not forwarded.
#[derive(Default)]
struct LastSent(Option<Option<MediaInfo>>);

impl LastSent {
    /// True (and remembers `next`) when `next` differs from what was last sent.
    fn changed(&mut self, next: &Option<MediaInfo>) -> bool {
        if self.0.as_ref() == Some(next) {
            return false;
        }
        self.0 = Some(next.clone());
        true
    }
}

/// Title, artist and album of a session, and its thumbnail when asked for.
struct Properties {
    title: String,
    artist: String,
    album: String,
    art: Option<Vec<u8>>,
}

/// What the watcher remembers of one session between reads.
#[derive(Default)]
struct Meta {
    title: String,
    artist: String,
    album: String,
}

/// One session the watcher follows, with the handlers registered on it
/// (removed when dropped).
struct Tracked {
    /// Never reused while the watcher runs.
    key: u64,
    /// The session's COM identity, to recognise it in a new list.
    identity: IUnknown,
    session: Session,
    /// The session's AppUserModelId, read once.
    app: String,
    /// `shown_playing` of the last status read: what the island displays.
    live: Option<bool>,
    /// `rank_playing` of the last status read: what ranking uses.
    rank_live: Option<bool>,
    status: &'static str,
    meta: Meta,
    /// The metadata was read (or tried) once.
    meta_tried: bool,
    properties_token: Option<i64>,
    playback_token: Option<i64>,
    timeline_token: Option<i64>,
}

impl Tracked {
    fn new(key: u64, identity: IUnknown, session: Session, signals: &Sender<Signal>) -> Self {
        let app = session
            .SourceAppUserModelId()
            .map(|id| id.to_string())
            .unwrap_or_default();
        let s = signals.clone();
        let properties_token = session
            .MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                let _ = s.send(Signal::Properties(key));
                Ok(())
            }))
            .ok();
        let s = signals.clone();
        let playback_token = session
            .PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                let _ = s.send(Signal::Playback(key));
                Ok(())
            }))
            .ok();
        Self {
            key,
            identity,
            session,
            app,
            live: None,
            rank_live: None,
            status: "Unreadable",
            meta: Meta::default(),
            meta_tried: false,
            properties_token,
            playback_token,
            timeline_token: None,
        }
    }

    /// Starts reporting timeline changes (the shown session only).
    fn subscribe_timeline(&mut self, signals: &Sender<Signal>) {
        if self.timeline_token.is_some() {
            return;
        }
        let (s, key) = (signals.clone(), self.key);
        self.timeline_token = self
            .session
            .TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                let _ = s.send(Signal::Timeline(key));
                Ok(())
            }))
            .ok();
    }

    fn unsubscribe_timeline(&mut self) {
        if let Some(token) = self.timeline_token.take() {
            let _ = self.session.RemoveTimelinePropertiesChanged(token);
        }
    }

    /// Re-reads the playback status.
    fn read_status(&mut self) {
        let status = self
            .session
            .GetPlaybackInfo()
            .and_then(|i| i.PlaybackStatus());
        self.status = status_name(&status);
        self.rank_live = rank_playing(&status);
        self.live = shown_playing(status);
    }

    /// Re-reads title, artist and album, and returns the thumbnail when
    /// `with_art`. A failed read keeps the previous values.
    fn read_meta(&mut self, with_art: bool) -> Option<Vec<u8>> {
        self.meta_tried = true;
        match read_properties(&self.session, with_art) {
            Ok(p) => {
                self.meta.title = p.title;
                self.meta.artist = p.artist;
                self.meta.album = p.album;
                p.art
            }
            Err(_) => None,
        }
    }

    /// `held`: the session is the shown one and inside its hold.
    fn candidate(&self, held: bool) -> Candidate {
        Candidate {
            key: self.key,
            app: self.app.clone(),
            live: ranked_live(self.rank_live, held),
            music: is_music(&self.app, &self.meta.artist, &self.meta.album),
        }
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        if let Some(token) = self.properties_token {
            let _ = self.session.RemoveMediaPropertiesChanged(token);
        }
        if let Some(token) = self.playback_token {
            let _ = self.session.RemovePlaybackInfoChanged(token);
        }
        self.unsubscribe_timeline();
    }
}

/// The sessions the watcher follows, for `send_command`: key, AppUserModelId
/// and the session. Updated before an `Event::Media` carrying a new key goes
/// out. Never held across a WinRT call that waits.
static SESSIONS: Mutex<Vec<(u64, String, Session)>> = Mutex::new(Vec::new());

fn publish_sessions(tracked: &[Tracked]) {
    let list = tracked
        .iter()
        .map(|t| (t.key, t.app.clone(), t.session.clone()))
        .collect();
    *SESSIONS.lock().unwrap_or_else(|e| e.into_inner()) = list;
}

/// What a refresh found.
struct Update {
    /// What the island should now show.
    info: Option<MediaInfo>,
    /// The shown session changed, or its properties were re-read: a new
    /// track may have started.
    new_track: bool,
}

/// State owned by the watcher thread.
struct Watcher {
    manager: Manager,
    manager_token: Option<i64>,
    /// `SessionsChanged`: a session opened or closed, which can change the
    /// current one without `CurrentSessionChanged` firing.
    sessions_token: Option<i64>,
    signals: Sender<Signal>,
    tracked: Vec<Tracked>,
    /// The key the next session gets; starts at 1 (0 means unknown).
    next_key: u64,
    /// The key of the session on show.
    shown: Option<u64>,
    /// The shown session stopped playing a moment ago and still ranks as
    /// playing until then (`next_hold`): its key and the deadline.
    hold: Option<(u64, Instant)>,
    /// The thumbnail of the shown session, and whose it is.
    art: Option<Vec<u8>>,
    art_for: Option<u64>,
    /// The last (session, status) pair, for the diagnosis log.
    status_seen: StatusSeen,
}

impl Watcher {
    fn new(manager: Manager, signals: Sender<Signal>) -> Self {
        let s = signals.clone();
        let manager_token = manager
            .CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                let _ = s.send(Signal::Session);
                Ok(())
            }))
            .ok();
        let s = signals.clone();
        let sessions_token = manager
            .SessionsChanged(&TypedEventHandler::new(move |_, _| {
                let _ = s.send(Signal::Session);
                Ok(())
            }))
            .ok();
        Self {
            manager,
            manager_token,
            sessions_token,
            signals,
            tracked: Vec::new(),
            next_key: 1,
            shown: None,
            hold: None,
            art: None,
            art_for: None,
            status_seen: StatusSeen::default(),
        }
    }

    /// Whether a signal still means something: its session is tracked (the
    /// timeline: shown). Late events of a session that is gone are dropped.
    fn accepts(&self, signal: Signal) -> bool {
        match signal {
            Signal::Session => true,
            Signal::Properties(key) | Signal::Playback(key) => {
                self.tracked.iter().any(|t| t.key == key)
            }
            Signal::Timeline(key) => self.shown == Some(key),
        }
    }

    /// Follows the manager's sessions again: known ones keep their key,
    /// new ones are subscribed, gone ones are dropped.
    fn relist(&mut self) {
        let Ok(sessions) = self.manager.GetSessions() else {
            return;
        };
        let Ok(size) = sessions.Size() else {
            return;
        };
        let mut old = std::mem::take(&mut self.tracked);
        let mut next = Vec::new();
        for i in 0..size {
            let Ok(session) = sessions.GetAt(i) else {
                continue;
            };
            let Ok(identity) = session.cast::<IUnknown>() else {
                continue;
            };
            if let Some(pos) = old.iter().position(|t| t.identity == identity) {
                next.push(old.remove(pos));
            } else {
                let key = self.next_key;
                self.next_key += 1;
                next.push(Tracked::new(key, identity, session, &self.signals));
            }
        }
        self.tracked = next;
        publish_sessions(&self.tracked);
    }

    /// The key of the session Windows calls current.
    fn current_key(&self) -> Option<u64> {
        let current = self.manager.GetCurrentSession().ok()?;
        let identity = current.cast::<IUnknown>().ok()?;
        self.tracked
            .iter()
            .find(|t| t.identity == identity)
            .map(|t| t.key)
    }

    /// Applies `refresh` and returns what the island should now show.
    fn refresh(&mut self, refresh: &Refresh) -> Update {
        if refresh.session {
            self.relist();
        }
        let before = self.shown;
        if before.is_some_and(|k| !self.tracked.iter().any(|t| t.key == k)) {
            self.shown = None;
        }
        // Metadata: new sessions and those whose own properties changed. The
        // thumbnail only for the shown one.
        let mut shown_reread = false;
        for t in &mut self.tracked {
            let asked = refresh.properties.contains(&t.key);
            if !asked && t.meta_tried {
                continue;
            }
            let is_shown = self.shown == Some(t.key);
            let art = t.read_meta(is_shown);
            if is_shown {
                self.art = art;
                self.art_for = Some(t.key);
                shown_reread = true;
            }
        }
        if refresh.rank {
            let shown_rank = |tracked: &[Tracked], shown: Option<u64>| {
                shown
                    .and_then(|k| tracked.iter().find(|t| t.key == k))
                    .and_then(|t| t.rank_live)
            };
            let was = shown_rank(&self.tracked, self.shown);
            for t in &mut self.tracked {
                t.read_status();
            }
            let now = Instant::now();
            let now_live = shown_rank(&self.tracked, self.shown);
            self.hold = next_hold(self.hold, self.shown, was, now_live, now);
            let hold = self.hold;
            let cands: Vec<Candidate> = self
                .tracked
                .iter()
                .map(|t| t.candidate(hold.is_some_and(|(k, until)| k == t.key && now < until)))
                .collect();
            let current = self.current_key();
            let chosen = choose(&cands, current, self.shown).map(|i| cands[i].key);
            if chosen != self.shown {
                self.hold = None;
            }
            if chosen != before {
                let live = cands.iter().filter(|c| c.live.is_some()).count();
                let app = chosen
                    .and_then(|k| cands.iter().find(|c| c.key == k))
                    .map_or("", |c| c.app.as_str());
                media_log::log_choice(app, live);
            }
            self.set_shown(chosen);
        }
        let shown_changed = self.shown != before;
        Update {
            info: self.info(),
            new_track: shown_changed || shown_reread,
        }
    }

    /// Makes `key` the shown session: the timeline handler moves to it and
    /// its thumbnail is read.
    fn set_shown(&mut self, key: Option<u64>) {
        if self.shown != key {
            if let Some(old) = self.shown
                && let Some(t) = self.tracked.iter_mut().find(|t| t.key == old)
            {
                t.unsubscribe_timeline();
            }
            self.shown = key;
        }
        let signals = self.signals.clone();
        let Some(t) = key.and_then(|k| self.tracked.iter_mut().find(|t| t.key == k)) else {
            self.art = None;
            self.art_for = None;
            return;
        };
        t.subscribe_timeline(&signals);
        if self.art_for != Some(t.key) {
            self.art = t.read_meta(true);
            self.art_for = Some(t.key);
        }
    }

    /// The shown session as the island displays it; logs its status when
    /// that changed.
    fn info(&mut self) -> Option<MediaInfo> {
        let shown = self
            .shown
            .and_then(|k| self.tracked.iter().find(|t| t.key == k));
        let Some(t) = shown else {
            if self.status_seen.changed("", "None") {
                media_log::log_status("", "None");
            }
            return None;
        };
        if self.status_seen.changed(&t.app, t.status) {
            media_log::log_status(&t.app, t.status);
        }
        let playing = t.live?;
        Some(MediaInfo {
            title: t.meta.title.clone(),
            artist: t.meta.artist.clone(),
            playing,
            art: if self.art_for == Some(t.key) {
                self.art.clone()
            } else {
                None
            },
            source_app: t.app.clone(),
            session_key: t.key,
        })
    }

    /// When the hold of the shown session runs out and the sessions must be
    /// ranked again.
    fn hold_deadline(&self) -> Option<Instant> {
        self.hold.map(|(_, until)| until)
    }

    /// The shown session's timeline; `None` without one or when it reports
    /// none.
    fn timeline(&self) -> Option<Timeline> {
        let key = self.shown?;
        let t = self.tracked.iter().find(|t| t.key == key)?;
        read_timeline(&t.session).ok().flatten()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(token) = self.manager_token {
            let _ = self.manager.RemoveCurrentSessionChanged(token);
        }
        if let Some(token) = self.sessions_token {
            let _ = self.manager.RemoveSessionsChanged(token);
        }
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// Whether a session with this playback status is playing; `None` when it is
/// closed or stopped and so should not be shown at all. An unreadable status
/// keeps the session on screen as paused.
fn shown_playing(status: Result<PlaybackStatus>) -> Option<bool> {
    match status {
        Ok(PlaybackStatus::Closed | PlaybackStatus::Stopped) => None,
        Ok(status) => Some(status == PlaybackStatus::Playing),
        Err(_) => Some(false),
    }
}

/// Whether a session with this status ranks as playing; `None` like
/// `shown_playing`. A session changing tracks (`Changing`) ranks as playing,
/// though the island displays it as paused, so a skip does not hand the
/// island to another playing session for a moment.
fn rank_playing(status: &Result<PlaybackStatus>) -> Option<bool> {
    match status {
        Ok(PlaybackStatus::Changing) => Some(true),
        Ok(PlaybackStatus::Closed | PlaybackStatus::Stopped) => None,
        Ok(status) => Some(*status == PlaybackStatus::Playing),
        Err(_) => Some(false),
    }
}

/// `rank_live` as ranking sees it: a held session that stopped playing still
/// ranks as playing. A closed or stopped one is never held.
fn ranked_live(rank_live: Option<bool>, held: bool) -> Option<bool> {
    match rank_live {
        Some(false) if held => Some(true),
        other => other,
    }
}

/// The hold of the shown session after a status read: `(key, until)`.
/// It starts when the shown session goes from playing to not playing (paused,
/// opened, unreadable), lasts `HOLD`, and ends early when it plays again,
/// closes or stops, or another session is shown.
fn next_hold(
    hold: Option<(u64, Instant)>,
    shown: Option<u64>,
    was: Option<bool>,
    now_live: Option<bool>,
    now: Instant,
) -> Option<(u64, Instant)> {
    let key = shown?;
    if now_live != Some(false) {
        return None;
    }
    if was == Some(true) {
        return Some((key, now + HOLD));
    }
    hold.filter(|&(k, until)| k == key && now < until)
}

/// The status's name for the diagnosis log.
fn status_name(status: &Result<PlaybackStatus>) -> &'static str {
    match status {
        Ok(PlaybackStatus::Playing) => "Playing",
        Ok(PlaybackStatus::Paused) => "Paused",
        Ok(PlaybackStatus::Stopped) => "Stopped",
        Ok(PlaybackStatus::Closed) => "Closed",
        Ok(PlaybackStatus::Changing) => "Changing",
        Ok(PlaybackStatus::Opened) => "Opened",
        Ok(_) => "Other",
        Err(_) => "Unreadable",
    }
}

/// Watches the system media sessions on a background thread and sends
/// `Event::Media` whenever what the island shows changes: once at start, then
/// on session, track and play/pause changes. Every session is ranked and the
/// best one is shown (`choose`). A changed timeline (a seek, a new track,
/// play/pause) follows as `Event::MediaTimeline`, without re-reading the art;
/// steady playback reported again is not sent (`timeline_moved`). The thread
/// blocks between events.
pub fn spawn_watcher(tx: Sender<Event>, wake: impl Fn() + Send + Sync + 'static) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let Ok(manager) = Manager::RequestAsync().and_then(|op| op.join()) else {
            return;
        };
        let (signals, signal_rx) = mpsc::channel();
        let mut watcher = Watcher::new(manager, signals);
        let mut last = LastSent::default();
        let mut last_timeline: Option<Timeline> = None;
        let mut refresh = Refresh::default();
        refresh.add(Signal::Session);
        loop {
            let update = watcher.refresh(&refresh);
            let info = update.info;
            let (present, playing) = (info.is_some(), info.as_ref().is_some_and(|m| m.playing));
            let mut sent = false;
            if last.changed(&info) {
                if tx.send(Event::Media(info)).is_err() {
                    return;
                }
                sent = true;
            }
            if refresh.timeline || sent || update.new_track {
                let timeline = if present { watcher.timeline() } else { None };
                let now = Instant::now();
                // After a new session or new properties (a new track) it goes
                // out even if unchanged, so the island pairs the new title
                // with it.
                let moved = timeline_moved(last_timeline.as_ref(), timeline.as_ref(), playing, now);
                if update.new_track || moved {
                    if tx.send(Event::MediaTimeline(timeline)).is_err() {
                        return;
                    }
                    last_timeline = timeline;
                    sent = true;
                }
            }
            if sent {
                wake();
            }
            let deadline = watcher.hold_deadline();
            refresh = next_refresh(&signal_rx, deadline, |signal| watcher.accepts(signal));
        }
    });
}

/// Blocks for the next signal `accept` lets through, then folds in any
/// already queued behind it. At `deadline` (a hold running out) it stops
/// waiting and asks for a re-rank.
fn next_refresh(
    signals: &Receiver<Signal>,
    deadline: Option<Instant>,
    accept: impl Fn(Signal) -> bool,
) -> Refresh {
    let mut refresh = Refresh::default();
    // The watcher holds a sender, so `recv` only returns once a signal arrives.
    // A burst of dropped signals (a closed session's) waits for the next one.
    while refresh == Refresh::default() {
        let received = match deadline {
            None => signals.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(at) => signals.recv_timeout(at.saturating_duration_since(Instant::now())),
        };
        let signal = match received {
            Ok(signal) => signal,
            Err(RecvTimeoutError::Timeout) => {
                refresh.rank = true;
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if accept(signal) {
            refresh.add(signal);
        }
    }
    for signal in signals.try_iter() {
        if accept(signal) {
            refresh.add(signal);
        }
    }
    refresh
}

/// The session a transport command is meant for: the one the island shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// The watcher's key of the shown session (`MediaInfo::session_key`); 0
    /// when nothing is shown.
    pub key: u64,
    /// Its AppUserModelId, for the log only.
    pub app: String,
}

/// Sends a transport command to the session with `target.key` (the one the
/// island shows) on a short-lived thread, so the caller never waits on WinRT.
/// Without such a session nothing is sent. `click`, when the command came
/// from one, goes to the diagnosis log. Errors are ignored.
pub fn send_command(cmd: MediaCommand, target: Target, click: Option<Click>) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let _ = try_send_command(&cmd, &target, click.as_ref());
    });
}

fn try_send_command(cmd: &MediaCommand, target: &Target, click: Option<&Click>) -> Result<bool> {
    let session = find_session(target.key);
    if let Some(click) = click {
        let outcome = Outcome::Sent {
            cmd,
            session_found: session.is_some(),
        };
        media_log::log_command(click, outcome, &target.app);
    }
    let Some(session) = session else {
        return Ok(false);
    };
    let op = match cmd {
        MediaCommand::Prev => session.TrySkipPreviousAsync()?,
        MediaCommand::Play => session.TryPlayAsync()?,
        MediaCommand::Pause => session.TryPauseAsync()?,
        MediaCommand::Next => session.TrySkipNextAsync()?,
    };
    op.join()
}

/// The tracked session with `key`, cloned out of the registry so the lock is
/// not held while the command runs.
fn find_session(key: u64) -> Option<Session> {
    let sessions = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let keys: Vec<u64> = sessions.iter().map(|(k, _, _)| *k).collect();
    pick_session(&keys, key).map(|i| sessions[i].2.clone())
}

/// Which of the tracked sessions (by key) gets a command meant for
/// `target_key`. Key 0 (unknown) and keys not tracked get none.
fn pick_session(keys: &[u64], target_key: u64) -> Option<usize> {
    if target_key == 0 {
        return None;
    }
    keys.iter().position(|&k| k == target_key)
}

/// Reads title, artist and album, and the thumbnail when `with_art`.
fn read_properties(session: &Session, with_art: bool) -> Result<Properties> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let art = if with_art {
        props
            .Thumbnail()
            .ok()
            .and_then(|thumbnail| read_thumbnail(&thumbnail).ok().flatten())
    } else {
        None
    };
    Ok(Properties {
        title: props.Title()?.to_string(),
        artist: props.Artist()?.to_string(),
        album: props
            .AlbumTitle()
            .map(|a| a.to_string())
            .unwrap_or_default(),
        art,
    })
}

/// 100 ns ticks between 1601-01-01 (WinRT `DateTime`) and 1970-01-01.
const TICKS_TO_UNIX: i64 = 116_444_736_000_000_000;

/// The session's timeline, positions relative to its start. `None` when it
/// reports no length and no position (players without timeline support).
fn read_timeline(session: &Session) -> Result<Option<Timeline>> {
    let t = session.GetTimelineProperties()?;
    let start = t.StartTime()?.Duration;
    let ms = |ticks: i64| u64::try_from(ticks.saturating_sub(start) / 10_000).unwrap_or(0);
    let position_ms = ms(t.Position()?.Duration);
    let end_ms = ms(t.EndTime()?.Duration);
    if position_ms == 0 && end_ms == 0 {
        return Ok(None);
    }
    let updated_ticks = t.LastUpdatedTime()?.UniversalTime;
    let age = Duration::from_millis(age_ms(now_ticks(), updated_ticks));
    // The rate the player reports (a faster or slower playback); unreadable
    // or odd is normal speed.
    let rate = session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackRate())
        .and_then(|r| r.Value())
        .map_or(1.0, |r| r as f32);
    let rate = if rate.is_finite() && rate > 0.0 {
        rate
    } else {
        1.0
    };
    Ok(Some(Timeline {
        position_ms,
        end_ms,
        updated: Instant::now().checked_sub(age).unwrap_or_else(Instant::now),
        rate,
    }))
}

/// The wall clock as WinRT `DateTime` ticks.
fn now_ticks() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    TICKS_TO_UNIX.saturating_add(i64::try_from(since.as_nanos() / 100).unwrap_or(i64::MAX))
}

/// How long ago `updated` was, in ms: 0 when unset or in the future, at most
/// a day (a stale stamp must not throw the position hours ahead).
fn age_ms(now_ticks: i64, updated_ticks: i64) -> u64 {
    if updated_ticks <= 0 {
        return 0;
    }
    let ms = now_ticks.saturating_sub(updated_ticks) / 10_000;
    u64::try_from(ms).unwrap_or(0).min(86_400_000)
}

/// The thumbnail's encoded image bytes; `None` when empty or over the size cap.
fn read_thumbnail(thumbnail: &IRandomAccessStreamReference) -> Result<Option<Vec<u8>>> {
    let stream = thumbnail.OpenReadAsync()?.join()?;
    let size = stream.Size()?;
    if !art_size_ok(size) {
        return Ok(None);
    }
    let reader = DataReader::CreateDataReader(&stream)?;
    let loaded = reader.LoadAsync(size as u32)?.join()?;
    let mut bytes = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(Some(bytes))
}

fn art_size_ok(size: u64) -> bool {
    size > 0 && size <= MAX_ART_BYTES
}

/// Diagnostic mode for `--probe-media`: prints every media update until Ctrl+C.
#[cfg(debug_assertions)]
pub fn probe() {
    let (tx, rx) = mpsc::channel();
    spawn_watcher(tx, || {});
    println!("Watching media sessions; press Ctrl+C to stop.");
    for event in rx {
        match event {
            Event::Media(None) => println!("None"),
            Event::Media(Some(m)) => println!(
                "{} | {} | playing={} | app={} | key={} | art={} bytes",
                m.title,
                m.artist,
                m.playing,
                if m.source_app.is_empty() {
                    "-"
                } else {
                    &m.source_app
                },
                m.session_key,
                m.art.as_ref().map_or(0, Vec::len)
            ),
            Event::MediaTimeline(Some(t)) => println!(
                "  timeline {} / {} ms, updated {} ms ago",
                t.position_ms,
                t.end_ms,
                t.updated.elapsed().as_millis()
            ),
            Event::MediaTimeline(None) => println!("  no timeline"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(title: &str, playing: bool, art: Option<Vec<u8>>) -> Option<MediaInfo> {
        Some(MediaInfo {
            title: title.to_string(),
            artist: "Artist".to_string(),
            playing,
            art,
            source_app: "Chrome".to_string(),
            session_key: 1,
        })
    }

    /// A live candidate; `playing` is its status.
    fn cand(key: u64, app: &str, playing: bool, music: bool) -> Candidate {
        Candidate {
            key,
            app: app.to_string(),
            live: Some(playing),
            music,
        }
    }

    fn closed(key: u64, app: &str) -> Candidate {
        Candidate {
            key,
            app: app.to_string(),
            live: None,
            music: true,
        }
    }

    /// The key `choose` picks.
    fn picked(cands: &[Candidate], current: Option<u64>, shown: Option<u64>) -> Option<u64> {
        choose(cands, current, shown).map(|i| cands[i].key)
    }

    #[test]
    fn playing_music_beats_a_paused_social_tab() {
        let music = is_music("Spotify.exe", "Queen", "A Night at the Opera");
        let insta = is_music("Chrome", "", "");
        let cands = [
            cand(1, "Chrome", false, insta),
            cand(2, "Spotify.exe", true, music),
        ];
        assert_eq!(picked(&cands, Some(1), Some(1)), Some(2));
    }

    #[test]
    fn playing_music_beats_a_playing_video() {
        let music = is_music("Chrome", "Queen", "Greatest Hits");
        // A video: the channel as artist, no album.
        let video = is_music("Chrome", "Some Channel", "");
        assert!(music && !video);
        let cands = [
            cand(1, "Chrome", true, video),
            cand(2, "Chrome", true, music),
        ];
        assert_eq!(picked(&cands, Some(1), Some(1)), Some(2));
    }

    #[test]
    fn a_paused_player_gives_way_to_a_playing_video_and_takes_over_again() {
        let paused = [
            cand(1, "Spotify.exe", false, true),
            cand(2, "Chrome", true, false),
        ];
        assert_eq!(picked(&paused, Some(1), Some(1)), Some(2));
        // The music plays again: back to it, though the video stays shown
        // and playing.
        let playing = [
            cand(1, "Spotify.exe", true, true),
            cand(2, "Chrome", true, false),
        ];
        assert_eq!(picked(&playing, Some(2), Some(2)), Some(1));
    }

    #[test]
    fn a_known_player_without_an_album_is_music() {
        assert!(is_music("Spotify.exe", "Artist", ""));
        assert!(is_music("Spotify.exe", "", ""));
        let cands = [
            cand(1, "Chrome", true, is_music("Chrome", "Channel", "")),
            cand(2, "Spotify.exe", true, is_music("Spotify.exe", "", "")),
        ];
        assert_eq!(picked(&cands, Some(1), Some(1)), Some(2));
    }

    #[test]
    fn a_browser_video_tab_is_not_music() {
        // What a live Chrome tab reports for a video: PlaybackType Music
        // (Chromium declares it for every tab), the channel as artist and no
        // album. Only artist and album count, so it is not music.
        for app in ["Chrome", "MSEdge", "308046B0AF4A39CB", "Brave", "Opera"] {
            assert!(!is_music(app, "Some Channel", ""), "{app}");
            assert!(!is_music(app, "", ""), "{app}");
            assert!(is_music(app, "Queen", "Greatest Hits"), "{app}");
        }
        // A player that is not a browser and fills in both is music.
        assert!(is_music("VideoLAN.VLC", "Queen", "Greatest Hits"));
    }

    #[test]
    fn spotify_beats_a_playing_youtube_video_whichever_windows_calls_current() {
        let video = is_music("Chrome", "Some Channel", "");
        let music = is_music("Spotify.exe", "Queen", "");
        let cands = [
            cand(1, "Chrome", true, video),
            cand(2, "Spotify.exe", true, music),
        ];
        assert_eq!(picked(&cands, Some(1), Some(1)), Some(2));
    }

    #[test]
    fn an_artist_alone_or_an_album_alone_is_not_music() {
        assert!(!is_music("Chrome", "Artist", ""));
        assert!(!is_music("Chrome", "Artist", "   "));
        assert!(!is_music("Chrome", "", "Album"));
        assert!(is_music("Chrome", "Artist", "Album"));
    }

    #[test]
    fn closed_and_stopped_sessions_are_never_chosen() {
        // Not even when it is the only one, or the current one.
        assert_eq!(picked(&[closed(1, "Spotify.exe")], Some(1), Some(1)), None);
        let cands = [closed(1, "Spotify.exe"), closed(2, "Chrome")];
        assert_eq!(picked(&cands, Some(2), Some(1)), None);
        // A live one wins over a current one that is closed.
        let cands = [closed(1, "Spotify.exe"), cand(2, "Chrome", false, false)];
        assert_eq!(picked(&cands, Some(1), Some(1)), Some(2));
    }

    #[test]
    fn a_tie_goes_to_the_current_session() {
        let cands = [
            cand(1, "Chrome", true, false),
            cand(2, "Chrome", true, false),
        ];
        assert_eq!(picked(&cands, Some(2), Some(1)), Some(2));
        assert_eq!(picked(&cands, Some(1), Some(2)), Some(1));
    }

    #[test]
    fn a_tie_without_a_current_session_keeps_the_one_shown() {
        let cands = [
            cand(1, "Chrome", true, false),
            cand(2, "Chrome", true, false),
        ];
        assert_eq!(picked(&cands, None, Some(2)), Some(2));
        assert_eq!(picked(&cands, None, Some(1)), Some(1));
        // The current one is a third session, not in the list.
        assert_eq!(picked(&cands, Some(3), Some(2)), Some(2));
        let reversed = [cands[1].clone(), cands[0].clone()];
        assert_eq!(picked(&reversed, None, Some(1)), Some(1));
    }

    #[test]
    fn a_full_tie_takes_the_first_in_the_list() {
        let cands = [cand(5, "A", false, false), cand(3, "B", false, false)];
        assert_eq!(choose(&cands, None, None), Some(0));
    }

    #[test]
    fn nothing_to_choose_from() {
        assert_eq!(choose(&[], None, None), None);
        assert_eq!(choose(&[], Some(1), Some(1)), None);
    }

    #[test]
    fn a_single_live_session_is_chosen() {
        let cands = [closed(1, "Spotify.exe"), cand(2, "Chrome", false, false)];
        assert_eq!(choose(&cands, None, None), Some(1));
    }

    #[test]
    fn known_players_match_any_case_and_browsers_do_not() {
        for app in [
            "Spotify.exe",
            "SPOTIFY",
            "AppleInc.AppleMusicWin_nzyj5cx40ttqa!App",
            "AppleInc.iTunes",
            "TIDAL",
            "Deezer.exe",
            "AmazonMusic.exe",
            "th-ch.YouTube-Music",
            "SoundCloud",
            "foobar2000.exe",
            "MusicBee.exe",
            "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic",
            "Plexamp.exe",
        ] {
            assert!(known_player(app), "{app}");
        }
        for app in [
            "Chrome",
            "MSEdge",
            "firefox.exe",
            "chrome.exe",
            "",
            "YouTube",
        ] {
            assert!(!known_player(app), "{app:?}");
        }
    }

    #[test]
    fn a_command_goes_to_the_session_with_the_shown_key() {
        // Two browser tabs share the id "Chrome"; Windows' current one is
        // key 9, but the island shows key 7.
        let tabs = [7, 9];
        assert_eq!(pick_session(&tabs, 7), Some(0));
        assert_eq!(pick_session(&tabs, 9), Some(1));
        assert_eq!(pick_session(&[3, 7, 9], 9), Some(2));
    }

    #[test]
    fn an_unknown_key_or_no_key_sends_nothing() {
        assert_eq!(pick_session(&[7, 9], 8), None);
        assert_eq!(pick_session(&[7, 9], 0), None);
        assert_eq!(pick_session(&[0, 7], 0), None);
        assert_eq!(pick_session(&[], 7), None);
    }

    #[test]
    fn a_session_without_an_id_is_still_targeted_by_key() {
        // The key identifies it, whatever its AppUserModelId (empty here).
        let sessions = [(4u64, ""), (5, "Chrome")];
        let keys: Vec<u64> = sessions.iter().map(|s| s.0).collect();
        assert_eq!(pick_session(&keys, 4), Some(0));
    }

    #[test]
    fn status_names_for_the_log() {
        assert_eq!(status_name(&Ok(PlaybackStatus::Playing)), "Playing");
        assert_eq!(status_name(&Ok(PlaybackStatus::Paused)), "Paused");
        assert_eq!(status_name(&Ok(PlaybackStatus::Closed)), "Closed");
        assert_eq!(
            status_name(&Err(windows::core::Error::empty())),
            "Unreadable"
        );
    }

    #[test]
    fn first_value_is_always_sent_even_none() {
        let mut last = LastSent::default();
        assert!(last.changed(&None));
        assert!(!last.changed(&None));
    }

    #[test]
    fn identical_info_is_not_resent() {
        let mut last = LastSent::default();
        assert!(last.changed(&info("A", true, Some(vec![1, 2]))));
        assert!(!last.changed(&info("A", true, Some(vec![1, 2]))));
    }

    #[test]
    fn play_state_title_and_late_art_count_as_changes() {
        let mut last = LastSent::default();
        assert!(last.changed(&info("A", true, None)));
        assert!(last.changed(&info("A", false, None)));
        assert!(last.changed(&info("B", false, None)));
        assert!(last.changed(&info("B", false, Some(vec![9]))));
        assert!(last.changed(&None));
    }

    #[test]
    fn another_session_with_the_same_text_counts_as_a_change() {
        let mut last = LastSent::default();
        let mut a = info("A", true, None);
        assert!(last.changed(&a));
        a.as_mut().unwrap().session_key = 2;
        assert!(last.changed(&a));
    }

    #[test]
    fn a_timeline_signal_rereads_only_the_timeline() {
        let mut r = Refresh::default();
        r.add(Signal::Timeline(1));
        assert_eq!(
            r,
            Refresh {
                timeline: true,
                ..Refresh::default()
            }
        );
    }

    #[test]
    fn a_playback_signal_reranks_and_rereads_the_timeline() {
        let mut r = Refresh::default();
        r.add(Signal::Playback(3));
        assert_eq!(
            r,
            Refresh {
                rank: true,
                timeline: true,
                ..Refresh::default()
            }
        );
    }

    #[test]
    fn new_properties_reread_that_session_and_rerank() {
        let mut r = Refresh::default();
        r.add(Signal::Properties(2));
        r.add(Signal::Properties(2));
        r.add(Signal::Properties(5));
        assert_eq!(r.properties, vec![2, 5]);
        assert!(r.rank && r.timeline && !r.session);
    }

    #[test]
    fn timeline_age_is_clamped() {
        let now = TICKS_TO_UNIX + 1_000_000_000;
        assert_eq!(age_ms(now, now - 25_000_000), 2_500);
        assert_eq!(age_ms(now, 0), 0, "unset");
        assert_eq!(age_ms(now, now + 10_000_000), 0, "in the future");
        assert_eq!(age_ms(now, 1), 86_400_000, "at most a day");
        // The wall clock is past 2023 in WinRT ticks.
        assert!(now_ticks() > TICKS_TO_UNIX + 1_700_000_000 * 10_000_000);
    }

    #[test]
    fn a_session_change_relists_reranks_and_rereads_the_timeline() {
        let mut r = Refresh::default();
        r.add(Signal::Playback(1));
        r.add(Signal::Session);
        assert_eq!(
            r,
            Refresh {
                session: true,
                properties: vec![],
                rank: true,
                timeline: true
            }
        );
    }

    #[test]
    fn coalesces_queued_signals() {
        let (tx, rx) = mpsc::channel();
        tx.send(Signal::Playback(1)).unwrap();
        tx.send(Signal::Properties(2)).unwrap();
        tx.send(Signal::Playback(2)).unwrap();
        tx.send(Signal::Timeline(1)).unwrap();
        let r = next_refresh(&rx, None, |_| true);
        assert_eq!(
            r,
            Refresh {
                session: false,
                properties: vec![2],
                rank: true,
                timeline: true
            }
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn signals_of_unknown_sessions_are_dropped() {
        let (tx, rx) = mpsc::channel();
        // Key 9 is gone: its events do not wake the watcher, the next
        // accepted one does.
        tx.send(Signal::Playback(9)).unwrap();
        tx.send(Signal::Properties(9)).unwrap();
        tx.send(Signal::Timeline(9)).unwrap();
        tx.send(Signal::Timeline(2)).unwrap();
        tx.send(Signal::Playback(9)).unwrap();
        let r = next_refresh(&rx, None, |s| matches!(s, Signal::Timeline(2)));
        assert_eq!(
            r,
            Refresh {
                timeline: true,
                ..Refresh::default()
            }
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn changing_tracks_ranks_as_playing_but_displays_paused() {
        let changing = Ok(PlaybackStatus::Changing);
        assert_eq!(rank_playing(&changing), Some(true));
        assert_eq!(shown_playing(changing), Some(false));
        assert_eq!(rank_playing(&Ok(PlaybackStatus::Playing)), Some(true));
        for status in [PlaybackStatus::Paused, PlaybackStatus::Opened] {
            assert_eq!(rank_playing(&Ok(status)), Some(false), "{status:?}");
        }
        assert_eq!(rank_playing(&Ok(PlaybackStatus::Closed)), None);
        assert_eq!(rank_playing(&Ok(PlaybackStatus::Stopped)), None);
        assert_eq!(
            rank_playing(&Err(windows::core::Error::empty())),
            Some(false)
        );
    }

    #[test]
    fn a_skip_on_the_shown_player_keeps_it_over_a_playing_video() {
        // Spotify (shown) reports Changing while a muted video plays.
        let spotify = ranked_live(rank_playing(&Ok(PlaybackStatus::Changing)), false);
        let cands = [
            Candidate {
                live: spotify,
                ..cand(1, "Spotify.exe", true, true)
            },
            cand(2, "Chrome", true, false),
        ];
        assert_eq!(picked(&cands, Some(2), Some(1)), Some(1));
    }

    #[test]
    fn a_held_session_ranks_as_playing_a_closed_one_never() {
        assert_eq!(ranked_live(Some(false), true), Some(true));
        assert_eq!(ranked_live(Some(false), false), Some(false));
        assert_eq!(ranked_live(Some(true), false), Some(true));
        assert_eq!(ranked_live(None, true), None);
    }

    #[test]
    fn the_hold_starts_when_the_shown_session_stops_playing() {
        let now = Instant::now();
        let hold = next_hold(None, Some(1), Some(true), Some(false), now);
        assert_eq!(hold, Some((1, now + HOLD)));
        // A later read inside the hold keeps it; it does not start again.
        let later = now + Duration::from_millis(500);
        assert_eq!(
            next_hold(hold, Some(1), Some(false), Some(false), later),
            hold
        );
        // It runs out.
        assert_eq!(
            next_hold(hold, Some(1), Some(false), Some(false), now + HOLD),
            None
        );
    }

    #[test]
    fn the_hold_ends_early() {
        let now = Instant::now();
        let hold = Some((1, now + HOLD));
        // Playing again, closed or stopped.
        assert_eq!(next_hold(hold, Some(1), Some(false), Some(true), now), None);
        assert_eq!(next_hold(hold, Some(1), Some(false), None, now), None);
        // Another session is shown, or none.
        assert_eq!(
            next_hold(hold, Some(2), Some(false), Some(false), now),
            None
        );
        assert_eq!(next_hold(hold, None, Some(true), Some(false), now), None);
        // A paused session that was not playing before gets no hold.
        assert_eq!(
            next_hold(None, Some(1), Some(false), Some(false), now),
            None
        );
        assert_eq!(next_hold(None, Some(1), None, Some(false), now), None);
    }

    #[test]
    fn a_short_pause_between_tracks_keeps_the_shown_player() {
        let now = Instant::now();
        let hold = next_hold(None, Some(1), Some(true), Some(false), now);
        let held = |at: Instant| hold.is_some_and(|(k, until)| k == 1 && at < until);
        let cands = |at: Instant| {
            [
                Candidate {
                    live: ranked_live(Some(false), held(at)),
                    ..cand(1, "Spotify.exe", false, true)
                },
                cand(2, "Chrome", true, false),
            ]
        };
        assert_eq!(picked(&cands(now), Some(2), Some(1)), Some(1));
        // Still paused after the hold: the playing video takes over.
        assert_eq!(picked(&cands(now + HOLD), Some(2), Some(1)), Some(2));
    }

    #[test]
    fn the_hold_running_out_asks_for_a_rerank() {
        let (_tx, rx) = mpsc::channel::<Signal>();
        let r = next_refresh(&rx, Some(Instant::now()), |_| true);
        assert_eq!(
            r,
            Refresh {
                rank: true,
                ..Refresh::default()
            }
        );
    }

    #[test]
    fn a_signal_before_the_deadline_is_taken_as_usual() {
        let (tx, rx) = mpsc::channel();
        tx.send(Signal::Timeline(1)).unwrap();
        let r = next_refresh(&rx, Some(Instant::now() + HOLD), |_| true);
        assert_eq!(
            r,
            Refresh {
                timeline: true,
                ..Refresh::default()
            }
        );
    }

    #[test]
    fn closed_or_stopped_session_shows_nothing() {
        assert_eq!(shown_playing(Ok(PlaybackStatus::Closed)), None);
        assert_eq!(shown_playing(Ok(PlaybackStatus::Stopped)), None);
    }

    #[test]
    fn live_session_reports_whether_it_plays() {
        assert_eq!(shown_playing(Ok(PlaybackStatus::Playing)), Some(true));
        for status in [
            PlaybackStatus::Paused,
            PlaybackStatus::Opened,
            PlaybackStatus::Changing,
        ] {
            assert_eq!(shown_playing(Ok(status)), Some(false), "{status:?}");
        }
        // An unreadable status keeps the session on screen, paused.
        assert_eq!(
            shown_playing(Err(windows::core::Error::empty())),
            Some(false)
        );
    }

    #[test]
    fn art_size_cap() {
        assert!(!art_size_ok(0));
        assert!(art_size_ok(1));
        assert!(art_size_ok(MAX_ART_BYTES));
        assert!(!art_size_ok(MAX_ART_BYTES + 1));
    }
}
