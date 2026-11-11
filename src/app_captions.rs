//! The island's side of live captions (`crate::captions`, `crate::caption_card`):
//! whether a session is on, its worker, the lines it has heard, the card's
//! snapshot (sent only when something it shows changed) and what the user's
//! clicks in the card do. A child of `app`, so it works on `App` directly.
//!
//! On for this run only, never at the start. The card's window procedure
//! never calls into `App`: it posts `WM_APP_CAPTION_CARD` and
//! `on_caption_card_msg` answers. The worker's words arrive as
//! `Event::Captions` through the event queue; its thread only sends and wakes
//! the island.
//!
//! Power: nothing here runs while captions are off. Paused, on battery or
//! waiting for a model there is no worker, no capture and no loaded model;
//! only the card stays. Captions have no timer of their own: the battery
//! comes with `check_battery` (a power message and the island's 30 s battery
//! tick, which runs `captions_sync` while captions are on), the model with
//! the download's own words.
//!
//! Caption text is held in memory only and is never logged.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::record::notice;
use super::{App, CAPTION_LINE_MS, TIMER_CAPTION_LINE, WM_APP_EVENT, poster};
use crate::caption_card::{self as card, CardContent, Change, Msg, OpenArgs};
use crate::captions::control::{self, CardStatus, Ctl};
use crate::captions::lines::Lines;
use crate::captions::live::Out;
use crate::captions::prefs::{CaptionPrefs, TextSize};
use crate::captions::worker::{self, Running, Spec, lang_from_u8, lang_to_u8};
use crate::clipboard::win;
use crate::config::UiFont;
use crate::i18n::{Key, Lang, text};
use crate::model::{Event, Notice, NoticeKind};
use crate::notch_monitor;
use crate::recorder::capture::Source;
use crate::sources::battery::read_battery;
use crate::ui::tray;
use crate::transcribe::output::Speaker;
use crate::transcribe::prefs::TrLang;
use crate::transcribe::{gpu, runner};

/// `CF_UNICODETEXT`.
const CF_UNICODETEXT: u32 = 13;

/// What the card was last sent, apart from the lines (which have their own
/// marks). Two that compare equal show the same thing: no update is sent.
#[derive(Clone, Debug, PartialEq)]
struct Sent {
    /// A failure carries its reason in words (`failed_why`), not its code.
    status: CardStatus,
    lang: TrLang,
    size: TextSize,
    palette: [u32; 3],
    font: UiFont,
    animations: bool,
    slow: bool,
    /// The sources that stopped for good, `[mic, system]`.
    lost: [bool; 2],
    labelled: bool,
    since: Option<Instant>,
    /// The card draws its own words in the language in effect.
    ui: Lang,
}

pub(super) struct CaptionState {
    pub(super) ctl: Ctl,
    /// The session's worker and its id; `None` while nothing runs (off,
    /// paused, on battery, no model, failed).
    worker: Option<(u64, Running)>,
    next_id: u64,
    lines: Lines,
    /// The caption language, read by the worker for every chunk.
    lang: Arc<AtomicU8>,
    /// The engine fell behind once in this session.
    slow: bool,
    /// When the header's clock started, moved forward by the time spent not
    /// running; `None` while nothing runs.
    since: Option<Instant>,
    /// How long the session ran before the worker last stopped.
    ran: Duration,
    /// The `Lines::generation` the card holds.
    sent_gen: u64,
    /// A final line or a gap came since the card last got the lines.
    finals_dirty: bool,
    /// A worker counts time from its own start: what its lines are moved by
    /// so they come after the ones already held (`rebase`).
    base_ms: u64,
    /// The sources the running worker lost, `[mic, system]`.
    lost: [bool; 2],
    sent: Option<Sent>,
    /// The text last sent to the notch, so a line the timer took away is not
    /// shown again until the text changes.
    notch_last: Option<String>,
    /// The worker said something since the card was last looked at
    /// (`captions_flush`, once per burst of events).
    dirty: bool,
}

impl Default for CaptionState {
    fn default() -> Self {
        CaptionState {
            ctl: Ctl::default(),
            worker: None,
            next_id: 0,
            lines: Lines::new(speakers_of(false)),
            lang: Arc::new(AtomicU8::new(lang_to_u8(TrLang::Id))),
            slow: false,
            since: None,
            ran: Duration::ZERO,
            sent_gen: 0,
            finals_dirty: false,
            base_ms: 0,
            lost: [false; 2],
            sent: None,
            notch_last: None,
            dirty: false,
        }
    }
}

impl CaptionState {
    /// A line, final or grey, is there to show.
    fn has_lines(&self) -> bool {
        !self.lines.finals().is_empty() || !self.lines.interim().is_empty()
    }

    fn status(&self) -> Option<CardStatus> {
        control::status(&self.ctl, self.has_lines())
    }
}

// ---- pure parts --------------------------------------------------------------

/// What the card's status button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    TurnOnTranskrip,
    Download,
    KeepGoing,
}

/// What the notch line needs after the lines changed.
#[derive(Debug, PartialEq)]
enum NotchStep {
    /// Nothing to change.
    Keep,
    /// Show this text and give it its eight seconds.
    Show(String),
    /// Take the line away.
    Clear,
}

/// `line` is the newest caption text (`None` with captions off), `last` the
/// text the notch was last given and `shown` whether it holds a line now.
/// The same text as last time shows nothing new: after the silence timer
/// took it away, only different words bring the line back.
fn notch_step(notch_on: bool, line: Option<&str>, last: Option<&str>, shown: bool) -> NotchStep {
    match line.filter(|t| notch_on && !t.is_empty()) {
        None if shown => NotchStep::Clear,
        None => NotchStep::Keep,
        Some(text) if last == Some(text) => NotchStep::Keep,
        Some(text) => NotchStep::Show(text.to_owned()),
    }
}

/// The notch line changed (`changed`): whether the island is drawn again for
/// it. Behind a fullscreen app the island is hidden; the state holds the line
/// and the redraw when fullscreen ends shows it.
fn redraws(changed: bool, fullscreen: bool) -> bool {
    changed && !fullscreen
}

/// A word of worker `id` is taken only while that worker is the session's:
/// a cancelled one still talks until it has ended.
fn accepts(current: Option<u64>, id: u64) -> bool {
    current == Some(id)
}

/// The charger was pulled while captions were heard: the one moment the
/// notice is shown.
fn battery_edge(before: Option<CardStatus>, after: Option<CardStatus>) -> bool {
    matches!(before, Some(CardStatus::Running | CardStatus::Listening))
        && after == Some(CardStatus::Battery)
}

fn action_of(status: &CardStatus) -> Option<Action> {
    match status {
        CardStatus::NeedsTranskrip => Some(Action::TurnOnTranskrip),
        CardStatus::NeedsModel => Some(Action::Download),
        CardStatus::Battery => Some(Action::KeepGoing),
        _ => None,
    }
}

/// The control state from what was just read. What the session decided
/// (`on`, the pause, "Tetap jalan", a failure) stays as in `prev`.
/// `reading` is `read_battery`'s: `None` (a desktop) is not on battery.
/// `downloading` counts only while the engine or the model is missing.
fn ctl_of(
    prev: &Ctl,
    prefs: &CaptionPrefs,
    transcript_on: bool,
    engine_ok: bool,
    model_ok: bool,
    downloading: Option<u8>,
    reading: Option<(u8, bool)>,
) -> Ctl {
    Ctl {
        on: prev.on,
        user_paused: prev.user_paused,
        on_battery: matches!(reading, Some((_, false))),
        battery_ok: prefs.on_battery,
        session_battery_ok: prev.session_battery_ok,
        transcript_on,
        engine_ok,
        model_ok,
        downloading: downloading.filter(|_| !(engine_ok && model_ok)),
        failed: prev.failed.clone(),
    }
}

/// Who speaks on each channel, as the worker lays them out: the PC audio
/// alone has no label; with the mic it is "Saya" then "Peserta".
fn speakers_of(mic: bool) -> Vec<Option<Speaker>> {
    if mic {
        vec![Some(Speaker::Me), Some(Speaker::Others)]
    } else {
        vec![None]
    }
}

/// Where a worker that starts now puts its first line: at the newest line
/// held, so everything it says comes after it.
fn base_of(lines: &Lines) -> u64 {
    lines.finals().back().map_or(0, |l| l.start_ms)
}

/// A worker's word with its times moved by `base_ms` (`base_of` when the
/// worker started).
fn rebase(out: Out, base_ms: u64) -> Out {
    match out {
        Out::Final(mut seg) => {
            seg.start_ms = seg.start_ms.saturating_add(base_ms);
            seg.end_ms = seg.end_ms.saturating_add(base_ms);
            Out::Final(seg)
        }
        other => other,
    }
}

/// Why captions stopped, after "Caption berhenti:". Only what the user can
/// do something about has words; the worker's other codes ("engine",
/// "model", ...) give none and the card says just that captions stopped.
fn failed_why(lang: Lang, kind: &str) -> String {
    match kind {
        "mic-denied" => text(lang, Key::RecMicDeniedTitle).to_owned(),
        "capture" => text(lang, Key::CapSourceLost).to_owned(),
        _ => String::new(),
    }
}

/// The snapshot for the card. `finals`: the final lines go with it (they
/// changed, or the card just opened). A lost source is a grey line after
/// the ones being heard.
fn content_of(sent: &Sent, lines: &Lines, finals: bool) -> CardContent {
    let mut interim: Vec<(Option<Speaker>, String)> = lines
        .interim()
        .into_iter()
        .map(|(speaker, line)| (speaker, line.to_owned()))
        .collect();
    for source in [Source::Mic, Source::System] {
        if sent.lost[source.index()] {
            let key = match source {
                Source::Mic => Key::CapMicLost,
                Source::System => Key::CapSourceLost,
            };
            interim.push((None, text(sent.ui, key).to_owned()));
        }
    }
    CardContent {
        status: sent.status.clone(),
        lines: if finals {
            Change::Set(Arc::new(lines.finals().iter().cloned().collect()))
        } else {
            Change::Keep
        },
        interim,
        labelled: sent.labelled,
        running_since: sent.since,
        lang: sent.lang,
        size: sent.size,
        slow_hint: sent.slow,
        palette: sent.palette,
        font: sent.font,
        animations: sent.animations,
    }
}

impl App {
    /// The hotkey, the tray item and the Rekam switch.
    pub(super) fn captions_toggle(&mut self) {
        self.captions_set(!self.cap.ctl.on);
    }

    /// The tray's entry and the Rekam switch read it.
    pub(super) fn captions_on(&self) -> bool {
        self.cap.ctl.on
    }

    /// The newest caption text (grey, else final) while captions are on.
    pub(super) fn caption_line(&self) -> Option<&str> {
        if !self.cap.ctl.on {
            return None;
        }
        self.cap.lines.latest()
    }

    /// Turns captions on (a new session: the card opens, the worker starts
    /// when it may) or off (the worker stops, the card closes, the lines
    /// are gone). The transcript queue waits while they are on.
    pub(super) fn captions_set(&mut self, on: bool) {
        if self.cap.ctl.on == on {
            return;
        }
        self.captions_switch(on);
        self.captions_notch_sync();
        tray::set_captions(self.cap.ctl.on);
        // The Rekam page's switch and an open Settings follow.
        self.refresh(Instant::now());
    }

    /// `captions_set`'s change itself.
    fn captions_switch(&mut self, on: bool) {
        if on {
            let lang = self
                .config
                .captions
                .language
                .unwrap_or(self.config.transcript.language);
            self.cap = CaptionState {
                next_id: self.cap.next_id,
                lines: Lines::new(speakers_of(self.config.captions.mic)),
                lang: Arc::new(AtomicU8::new(lang_to_u8(lang))),
                ..CaptionState::default()
            };
            self.cap.ctl.on = true;
            self.cap.ctl = self.captions_ctl();
            if !self.captions_open_card() {
                // Without its card there is no way to see or close them.
                self.cap.ctl.on = false;
                return;
            }
            self.transcribe_sync_hold();
            self.captions_sync();
        } else {
            self.captions_halt();
            card::close();
            self.cap = CaptionState {
                next_id: self.cap.next_id,
                ..CaptionState::default()
            };
            // A queued transcript job may start now.
            self.transcribe_sync_hold();
        }
    }

    /// "Mic juga" changed: a running worker has the old channels, so it stops
    /// (without waiting) and the sync starts a fresh one. Nothing while
    /// captions are off.
    pub(super) fn captions_restart(&mut self) {
        self.captions_halt();
        self.captions_sync();
    }

    /// The card's pause button. Paused, nothing runs; the lines stay and
    /// the queue keeps waiting. Resuming starts a fresh worker.
    pub(super) fn captions_pause(&mut self, paused: bool) {
        if !self.cap.ctl.on || self.cap.ctl.user_paused == paused {
            return;
        }
        self.cap.ctl.user_paused = paused;
        self.captions_sync();
    }

    /// A power message (`check_battery`): on battery captions stop, back on
    /// the charger they go on by themselves.
    pub(super) fn captions_power(&mut self) {
        self.captions_sync();
    }

    /// Reads again what captions depend on (Transkrip, the engine and the
    /// model, a download, the battery), starts or stops the worker to match
    /// and tells the card. Nothing while captions are off.
    pub(super) fn captions_sync(&mut self) {
        if !self.cap.ctl.on {
            return;
        }
        let before = self.cap.status();
        self.cap.ctl = self.captions_ctl();
        let run = control::should_run(&self.cap.ctl);
        if run && self.cap.worker.is_none() {
            self.captions_start();
        } else if !run {
            self.captions_halt();
        }
        if battery_edge(before, self.cap.status()) {
            let lang = self.config.language;
            let title = text(lang, Key::CapBatteryNotice).to_owned();
            let n = notice(
                "cap-battery",
                lang,
                title,
                String::new(),
                NoticeKind::RecInfo,
            );
            self.apply_now(Event::Notice(n));
        }
        self.captions_card_sync();
    }

    fn captions_ctl(&self) -> Ctl {
        let model = self.config.transcript.model_for(gpu::has_hardware_gpu());
        let store = &self.tr.store;
        ctl_of(
            &self.cap.ctl,
            &self.config.captions,
            self.config.transcript.enabled,
            store.engine_installed(),
            store.model_installed(model),
            self.transcribe_download_percent(model),
            read_battery(),
        )
    }

    /// A fresh worker for the session. Its thread only sends its words into
    /// the event queue and wakes the island.
    fn captions_start(&mut self) {
        let mic = self.config.captions.mic;
        // "Mic juga" changed since the lines began: the labels no longer
        // fit them, so the session's lines start over.
        if self.cap.lines.labelled() != mic {
            self.cap.lines = Lines::new(speakers_of(mic));
            self.cap.finals_dirty = true;
        }
        let gpu = gpu::has_hardware_gpu();
        let model = self.config.transcript.model_for(gpu);
        let store = &self.tr.store;
        let rec = &self.config.recording;
        let spec = Spec {
            engine_dir: store.engine_dir(),
            model_path: store.model_path(model),
            vad_path: store.vad_installed().then(|| store.vad_path()),
            gpu,
            threads: runner::default_threads(),
            mic,
            devices: [rec.mic_device.clone(), rec.system_device.clone()],
            language: self.cap.lang.clone(),
        };
        self.cap.next_id += 1;
        let id = self.cap.next_id;
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let spawned = worker::spawn(spec, move |ev| {
            let _ = tx.send(Event::Captions(id, ev));
            wake();
        });
        match spawned {
            Ok(running) => {
                let now = Instant::now();
                self.cap.worker = Some((id, running));
                self.cap.since = Some(now.checked_sub(self.cap.ran).unwrap_or(now));
                self.cap.base_ms = base_of(&self.cap.lines);
            }
            Err(e) => {
                eprintln!("vale: caption thread did not start: {e}");
                self.cap.ctl.failed = Some("thread".into());
            }
        }
    }

    /// Stops the worker without waiting for it (a model that is loading
    /// takes seconds): it ends on its own and its last words are an old
    /// worker's. What it was still hearing is dropped; the lines stay.
    fn captions_halt(&mut self) {
        let Some((_, running)) = self.cap.worker.take() else {
            return;
        };
        running.cancel();
        if let Some(since) = self.cap.since.take() {
            self.cap.ran = since.elapsed();
        }
        self.cap.lost = [false; 2];
        for channel in 0..2 {
            self.cap.lines.apply(Out::ClearInterim { channel });
        }
    }

    /// Quitting: the worker is told to stop, not waited for.
    pub(super) fn captions_shutdown(&mut self) {
        self.captions_halt();
    }

    /// A word of a caption worker, from the event queue. One of an older
    /// worker is ignored.
    pub(super) fn on_captions(&mut self, id: u64, event: worker::Event) {
        if !accepts(self.cap.worker.as_ref().map(|w| w.0), id) {
            return;
        }
        self.cap.dirty = true;
        match event {
            // Again after a GPU that failed: the capture was opened anew
            // and counts from zero again.
            worker::Event::Started { .. } => {
                self.cap.base_ms = base_of(&self.cap.lines);
                self.cap.lost = [false; 2];
            }
            worker::Event::Out(Out::Slow) => self.cap.slow = true,
            worker::Event::Out(out) => {
                self.cap.finals_dirty |= matches!(out, Out::Final(_) | Out::Gap);
                self.cap.lines.apply(rebase(out, self.cap.base_ms));
            }
            worker::Event::SourceLost(source) => self.cap.lost[source.index()] = true,
            worker::Event::Failed(kind) => self.captions_failed(kind),
            // Without a failure before it: the thread ended on its own.
            worker::Event::Ended => self.captions_failed("ended".into()),
        }
    }

    /// The session cannot go on: it stays stopped (the card says so) until
    /// captions are turned off and on again. Only the kind is logged.
    fn captions_failed(&mut self, kind: String) {
        eprintln!("vale: captions stopped ({kind})");
        self.captions_halt();
        self.cap.ctl.failed = Some(kind);
    }

    /// After a burst of events: the card follows what the worker said, once.
    pub(super) fn captions_flush(&mut self) {
        if std::mem::take(&mut self.cap.dirty) {
            self.captions_card_sync();
            // Caption events do not redraw by themselves (the burst's
            // refresh skips them): the notch line asks for its own.
            let changed = self.captions_notch_sync();
            if redraws(changed, self.fullscreen) {
                self.refresh(Instant::now());
            }
        }
    }

    /// Brings the notch's caption line in line with the newest text and the
    /// "Caption di notch" setting: a new text shows (and re-arms the one-shot
    /// that takes it away after `CAPTION_LINE_MS` of silence), captions or
    /// the setting off clear it and kill the timer. True when the state
    /// changed (the caller redraws). The text is never logged.
    pub(super) fn captions_notch_sync(&mut self) -> bool {
        let line = self.caption_line().map(str::to_owned);
        let step = notch_step(
            self.config.captions.notch,
            line.as_deref(),
            self.cap.notch_last.as_deref(),
            self.state.caption_line().is_some(),
        );
        match step {
            NotchStep::Keep => {
                if line.is_none() || !self.config.captions.notch {
                    self.cap.notch_last = None;
                }
                false
            }
            NotchStep::Clear => {
                self.cap.notch_last = None;
                self.captions_kill_line_timer();
                self.apply(Event::CaptionLine(None), Instant::now());
                true
            }
            NotchStep::Show(text) => {
                self.cap.notch_last = Some(text.clone());
                self.apply(Event::CaptionLine(Some(text)), Instant::now());
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_CAPTION_LINE, CAPTION_LINE_MS, None);
                }
                true
            }
        }
    }

    fn captions_kill_line_timer(&self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CAPTION_LINE);
        }
    }

    /// `TIMER_CAPTION_LINE`: eight seconds without new text, the line goes.
    pub(super) fn on_caption_line_timer(&mut self) {
        self.captions_kill_line_timer();
        if self.state.caption_line().is_some() {
            let now = Instant::now();
            self.apply(Event::CaptionLine(None), now);
            if redraws(true, self.fullscreen) {
                self.refresh(now);
            }
        }
    }

    fn captions_sent(&self) -> Option<Sent> {
        let ui = self.config.language;
        let status = match self.cap.status()? {
            CardStatus::Failed(kind) => CardStatus::Failed(failed_why(ui, &kind)),
            status => status,
        };
        Some(Sent {
            status,
            lang: lang_from_u8(self.cap.lang.load(Ordering::Relaxed)),
            size: self.config.captions.size,
            palette: crate::lyrics_card::fallback_palette(),
            font: self.config.font,
            animations: self.animations,
            slow: self.cap.slow,
            lost: self.cap.lost,
            labelled: self.cap.lines.labelled(),
            since: self.cap.since,
            ui,
        })
    }

    fn captions_open_card(&mut self) -> bool {
        let Some(sent) = self.captions_sent() else {
            return false;
        };
        let notch = notch_monitor::resolve(&self.config.notch_monitor, &self.displays);
        let (monitors, notch) = crate::lyrics_card::monitors_of(&self.displays, notch);
        let args = OpenArgs {
            island: self.hwnd,
            saved: self.config.captions.place,
            monitors,
            notch,
            demo: false,
        };
        if !card::open(args, content_of(&sent, &self.cap.lines, true)) {
            return false;
        }
        self.captions_sent_now(sent);
        true
    }

    fn captions_sent_now(&mut self, sent: Sent) {
        self.cap.sent = Some(sent);
        self.cap.sent_gen = self.cap.lines.generation();
        self.cap.finals_dirty = false;
    }

    /// Sends the card a snapshot when the lines, the status, the language,
    /// the size, the colours or the font changed, and only then. Nothing
    /// while captions are off.
    pub(super) fn captions_card_sync(&mut self) {
        if !self.cap.ctl.on {
            return;
        }
        let Some(sent) = self.captions_sent() else {
            return;
        };
        let finals = self.cap.finals_dirty;
        if !finals
            && self.cap.sent_gen == self.cap.lines.generation()
            && self.cap.sent.as_ref() == Some(&sent)
        {
            return;
        }
        card::update(content_of(&sent, &self.cap.lines, finals));
        self.captions_sent_now(sent);
    }

    /// What the user did in the card.
    pub(super) fn on_caption_card_msg(&mut self, code: usize, value: isize) {
        let Some(msg) = Msg::from_code(code) else {
            return;
        };
        if !self.cap.ctl.on {
            return;
        }
        match msg {
            Msg::Close => self.captions_set(false),
            Msg::Pause => self.captions_pause(true),
            Msg::Resume => self.captions_pause(false),
            Msg::Copy => self.captions_copy(),
            Msg::Lang => {
                let now = lang_from_u8(self.cap.lang.load(Ordering::Relaxed));
                let next = control::next_lang(now);
                self.cap.lang.store(lang_to_u8(next), Ordering::Relaxed);
                self.config.captions.language = Some(next);
                self.save_config();
                self.captions_card_sync();
            }
            Msg::Size => {
                let size = self.config.captions.size;
                let next = size.step(value != 0);
                if next != size {
                    self.config.captions.size = next;
                    self.save_config();
                    self.captions_card_sync();
                }
            }
            Msg::Placed => {
                if let Some(place) = card::take_placed() {
                    self.config.captions.place = Some(place);
                    self.save_config();
                }
            }
            Msg::Action => self.captions_action(),
        }
    }

    /// The status button: which one it is follows from the status the card
    /// was sent.
    fn captions_action(&mut self) {
        let Some(action) = self.cap.sent.as_ref().and_then(|s| action_of(&s.status)) else {
            return;
        };
        match action {
            Action::TurnOnTranskrip => {
                self.transcribe_set_enabled(true);
                self.captions_download_missing();
            }
            Action::Download => self.captions_download_missing(),
            Action::KeepGoing => self.cap.ctl.session_battery_ok = true,
        }
        self.captions_sync();
    }

    /// Starts the Transkrip download of what captions lack (the engine, the
    /// active model). Only from the card's button.
    fn captions_download_missing(&mut self) {
        let model = self.config.transcript.model_for(gpu::has_hardware_gpu());
        let store = &self.tr.store;
        if !store.engine_installed() || !store.model_installed(model) {
            self.transcribe_download(model);
        }
    }

    /// "Salin": the session's final lines as plain text, as the island's
    /// own copy (the clipboard history does not take it again).
    fn captions_copy(&mut self) {
        let lang = self.config.language;
        let body = self
            .cap
            .lines
            .copy_text(text(lang, Key::CapMe), text(lang, Key::CapOthers));
        if body.is_empty() {
            return;
        }
        let Some(sequence) = win::write(self.hwnd, &[(CF_UNICODETEXT, win::utf16z(&body))]) else {
            return;
        };
        self.clip.own_sequence = Some(sequence);
        self.apply_now(Event::Notice(Notice {
            key: "cap-copied".into(),
            source: text(lang, Key::CapTitle).into(),
            title: text(lang, Key::ClipCopied).into(),
            body: String::new(),
            kind: NoticeKind::Copied,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::output::{Segment, Speaker};

    #[test]
    fn events_of_an_old_worker_are_ignored() {
        assert!(accepts(Some(3), 3));
        // A cancelled worker still talks until it has ended.
        assert!(!accepts(Some(3), 2));
        assert!(!accepts(Some(3), 4));
        // Nothing runs (paused, off, failed): every word is an old one.
        assert!(!accepts(None, 3));
        assert!(!accepts(None, 0));
    }

    #[test]
    fn the_notch_line_follows_the_newest_text_and_the_setting() {
        use NotchStep::*;
        // A new text shows; the same one again does not restart anything.
        assert_eq!(notch_step(true, Some("halo"), None, false), Show("halo".into()));
        assert_eq!(notch_step(true, Some("halo"), Some("halo"), true), Keep);
        assert_eq!(
            notch_step(true, Some("halo dunia"), Some("halo"), true),
            Show("halo dunia".into())
        );
        // The silence timer took the line away: the same text stays away,
        // other words bring it back.
        assert_eq!(notch_step(true, Some("halo"), Some("halo"), false), Keep);
        assert_eq!(
            notch_step(true, Some("lagi"), Some("halo"), false),
            Show("lagi".into())
        );
        // The setting off, or captions off (no line), clears what shows.
        assert_eq!(notch_step(false, Some("halo"), Some("halo"), true), Clear);
        assert_eq!(notch_step(true, None, Some("halo"), true), Clear);
        assert_eq!(notch_step(false, Some("halo"), None, false), Keep);
        assert_eq!(notch_step(true, None, None, false), Keep);
        assert_eq!(notch_step(true, Some(""), None, false), Keep);
    }

    #[test]
    fn the_notch_line_is_not_redrawn_behind_a_fullscreen_app() {
        assert!(redraws(true, false));
        // Hidden: the state has the line for when fullscreen ends.
        assert!(!redraws(true, true));
        assert!(!redraws(false, false));
        assert!(!redraws(false, true));
    }

    #[test]
    fn battery_notice_only_on_the_running_to_battery_edge() {
        use CardStatus::*;
        assert!(battery_edge(Some(Running), Some(Battery)));
        assert!(battery_edge(Some(Listening), Some(Battery)));
        // Turned on while on battery, or still on battery: the card says it.
        assert!(!battery_edge(None, Some(Battery)));
        assert!(!battery_edge(Some(Battery), Some(Battery)));
        // The user's pause came first; nothing was running to stop.
        assert!(!battery_edge(Some(Paused), Some(Battery)));
        assert!(!battery_edge(Some(NeedsModel), Some(Battery)));
        // Back on the charger, or any other change.
        assert!(!battery_edge(Some(Battery), Some(Running)));
        assert!(!battery_edge(Some(Running), Some(Paused)));
        assert!(!battery_edge(Some(Running), None));
    }

    #[test]
    fn the_action_button_per_status() {
        use CardStatus::*;
        assert_eq!(action_of(&NeedsTranskrip), Some(Action::TurnOnTranskrip));
        assert_eq!(action_of(&NeedsModel), Some(Action::Download));
        assert_eq!(action_of(&Battery), Some(Action::KeepGoing));
        for none in [
            Listening,
            Running,
            Paused,
            Downloading(40),
            Failed("engine".into()),
        ] {
            assert_eq!(action_of(&none), None, "{none:?}");
        }
    }

    #[test]
    fn ctl_from_inputs() {
        let prefs = CaptionPrefs::default();
        let prev = Ctl {
            on: true,
            user_paused: true,
            session_battery_ok: true,
            failed: Some("engine".into()),
            ..Ctl::default()
        };
        let c = ctl_of(&prev, &prefs, true, true, true, None, Some((80, false)));
        // What the session decided stays.
        assert!(c.on && c.user_paused && c.session_battery_ok);
        assert_eq!(c.failed.as_deref(), Some("engine"));
        // What was read comes new.
        assert!(c.transcript_on && c.engine_ok && c.model_ok && c.on_battery);
        assert!(!c.battery_ok);
        assert_eq!(c.downloading, None);

        // On the charger, and a desktop (no battery): not on battery.
        assert!(!ctl_of(&prev, &prefs, true, true, true, None, Some((80, true))).on_battery);
        assert!(!ctl_of(&prev, &prefs, true, true, true, None, None).on_battery);

        // The setting that lets captions run on battery.
        let keep = CaptionPrefs {
            on_battery: true,
            ..CaptionPrefs::default()
        };
        assert!(ctl_of(&prev, &keep, true, true, true, None, None).battery_ok);

        // A download counts only while something captions need is missing:
        // another model coming in must not stop captions that run.
        let c = ctl_of(&prev, &prefs, true, true, false, Some(40), None);
        assert_eq!((c.model_ok, c.downloading), (false, Some(40)));
        let c = ctl_of(&prev, &prefs, true, false, true, Some(7), None);
        assert_eq!((c.engine_ok, c.downloading), (false, Some(7)));
        assert_eq!(
            ctl_of(&prev, &prefs, true, true, true, Some(40), None).downloading,
            None
        );
        assert!(!ctl_of(&prev, &prefs, false, true, true, None, None).transcript_on);
    }

    fn seg(start_ms: u64, end_ms: u64) -> Segment {
        Segment {
            start_ms,
            end_ms,
            speaker: Some(Speaker::Me),
            text: "x".into(),
        }
    }

    #[test]
    fn a_new_worker_s_lines_come_after_the_ones_before() {
        // A worker counts from its own start: its lines are moved behind
        // what the card already holds.
        assert_eq!(
            rebase(Out::Final(seg(1_000, 2_500)), 60_000),
            Out::Final(seg(61_000, 62_500))
        );
        assert_eq!(rebase(Out::Final(seg(5, 9)), 0), Out::Final(seg(5, 9)));
        // Nothing else carries a time.
        assert_eq!(rebase(Out::Gap, 60_000), Out::Gap);
        let grey = Out::Interim {
            channel: 1,
            text: "y".into(),
        };
        assert_eq!(rebase(grey.clone(), 60_000), grey);

        // In `Lines`: after a pause the fresh worker starts at 0 again.
        use crate::captions::lines::Lines;
        let mut lines = Lines::new(vec![None]);
        let plain = |start_ms| {
            Out::Final(Segment {
                start_ms,
                end_ms: start_ms + 1,
                speaker: None,
                text: start_ms.to_string(),
            })
        };
        lines.apply(plain(10_000));
        lines.apply(plain(50_000));
        let base = base_of(&lines);
        assert_eq!(base, 50_000);
        lines.apply(rebase(plain(2_000), base));
        let starts: Vec<u64> = lines.finals().iter().map(|l| l.start_ms).collect();
        assert_eq!(starts, [10_000, 50_000, 52_000]);
        assert_eq!(base_of(&Lines::new(vec![None])), 0);
    }

    #[test]
    fn a_failure_is_said_in_words_never_as_a_code() {
        for lang in [Lang::Id, Lang::En] {
            // The codes of the worker never show.
            for kind in ["engine", "model", "vad", "thread", "ended", "anything"] {
                assert_eq!(failed_why(lang, kind), "", "{kind}");
            }
            assert!(!failed_why(lang, "mic-denied").is_empty());
            assert!(!failed_why(lang, "capture").is_empty());
        }
        assert_eq!(
            failed_why(Lang::En, "mic-denied"),
            "Windows blocked the mic"
        );
        assert_eq!(failed_why(Lang::En, "capture"), "PC audio lost");
    }

    #[test]
    fn mic_too_gives_two_labelled_channels() {
        assert_eq!(speakers_of(false), [None]);
        assert_eq!(
            speakers_of(true),
            [Some(Speaker::Me), Some(Speaker::Others)]
        );
    }

    #[test]
    fn a_new_state_is_off_and_holds_nothing() {
        let s = CaptionState::default();
        assert!(!s.ctl.on && s.worker.is_none() && s.since.is_none());
        assert!(!s.slow && s.lines.finals().is_empty());
        assert!(!crate::captions::control::holds_queue(&s.ctl));
        assert!(!s.has_lines() && s.status().is_none());
    }

    fn sent(status: CardStatus) -> Sent {
        Sent {
            status,
            lang: TrLang::Id,
            size: TextSize::Medium,
            palette: [1, 2, 3],
            font: UiFont::Sans,
            animations: true,
            slow: false,
            lost: [false; 2],
            labelled: true,
            since: None,
            ui: Lang::En,
        }
    }

    #[test]
    fn the_snapshot_carries_the_final_lines_only_when_asked() {
        use crate::captions::lines::Lines;
        let mut lines = Lines::new(speakers_of(true));
        lines.apply(Out::Final(seg(0, 900)));
        lines.apply(Out::Interim {
            channel: 1,
            text: "grey".into(),
        });
        let s = sent(CardStatus::Running);

        let all = content_of(&s, &lines, true);
        let Change::Set(finals) = &all.lines else {
            panic!("the final lines were asked for");
        };
        assert_eq!(finals.len(), 1);
        assert_eq!(all.interim, [(Some(Speaker::Others), "grey".to_owned())]);
        assert!(all.labelled && !all.slow_hint && all.running_since.is_none());

        // Only a grey line moved: the card keeps the lines it has.
        let kept = content_of(&s, &lines, false);
        assert!(matches!(kept.lines, Change::Keep));
        assert_eq!(kept.interim.len(), 1);
    }

    #[test]
    fn a_lost_source_is_a_grey_line_in_the_language_in_effect() {
        use crate::captions::lines::Lines;
        let lines = Lines::new(speakers_of(true));
        let mut s = sent(CardStatus::Listening);
        assert!(content_of(&s, &lines, false).interim.is_empty());

        s.lost = [true, true];
        assert_eq!(
            content_of(&s, &lines, false).interim,
            [
                (None, "Mic lost".to_owned()),
                (None, "PC audio lost".to_owned())
            ]
        );
        s.lost = [false, true];
        s.ui = Lang::Id;
        assert_eq!(
            content_of(&s, &lines, false).interim,
            [(None, "Suara PC terputus".to_owned())]
        );
    }

    #[test]
    fn the_card_is_sent_again_only_when_what_it_shows_differs() {
        let a = sent(CardStatus::Running);
        assert_eq!(a, a.clone());
        let differs: [fn(&mut Sent); 8] = [
            |s| s.status = CardStatus::Paused,
            |s| s.lang = TrLang::En,
            |s| s.size = TextSize::Large,
            |s| s.palette = [9, 2, 3],
            |s| s.slow = true,
            |s| s.lost = [false, true],
            |s| s.since = Some(Instant::now()),
            |s| s.ui = Lang::Id,
        ];
        for change in differs {
            let mut b = a.clone();
            change(&mut b);
            assert_ne!(a, b);
        }
    }
}
