//! Audio capture for recordings: the microphone, the system audio (WASAPI
//! loopback on an output) or both, each on the device the user chose or on the
//! Windows default, delivered as mono `f32`
//! packets with a QPC timestamp to an [`AudioSink`]. The recorder session
//! (`recorder::Recorder`) and the screen recorder (track C) consume this.
//!
//! # Contract
//!
//! - [`Capture::start`] spawns one thread, `rec-capture` (COM MTA), opens the
//!   requested sources and returns once they are open. It fails only when no
//!   requested source could be opened; a source that fails while another one
//!   works is reported through [`AudioSink::lost`] right after the start.
//! - [`AudioSink::audio`] is called on the capture thread for every packet:
//!   `mono` is the packet down-mixed to one channel at the device's own mix
//!   rate `rate` (often 48000, but 44100 and others happen, and the rate can
//!   change when the default device changes), samples in -1..=1. `qpc` is the
//!   time of the packet's first frame in **100 ns units of the performance
//!   counter** (what `IAudioCaptureClient::GetBuffer` reports, not raw QPC
//!   ticks); [`qpc_now`] (`u64`) and [`qpc_hns`] (`i64`) read the same
//!   clock. Packets of one source are in order; the two sources interleave
//!   freely. The loopback source sends
//!   nothing while nothing plays: a gap in `qpc` is silence.
//! - Each source has a choice (`Choices`, shared with the caller): the id of
//!   an endpoint, or `None` to follow the Windows default. A source on the
//!   default whose default device changes is reopened on the new default
//!   and reported through [`AudioSink::switched`] (`fell_back: false`). A
//!   chosen device that is missing at the start, or goes away while
//!   recording, falls back to the default for the rest of the recording,
//!   reported with `fell_back: true`; the choice itself is left alone.
//!   [`Capture::set_device`] changes a choice and the switch happens at once,
//!   reported with `manual: true`. A source that fails (the device is unplugged,
//!   `AUDCLNT_E_DEVICE_INVALIDATED`, or the audio service restarts) is
//!   reopened for up to [`RETRY_FOR`]; when that does
//!   not work, [`AudioSink::lost`] is called once and that source stays off.
//!   The other source keeps running. A microphone refused by the Windows
//!   privacy setting is [`CaptureError::MicDenied`], never retried.
//! - [`Capture::stop`] (or dropping the `Capture`) signals the thread, which
//!   delivers the packets still buffered, releases the devices and ends;
//!   `stop` returns after the thread has ended. No thread or timer exists
//!   before `start` or after `stop`. When every source is lost for good the
//!   thread ends by itself (no more 100 ms wakes); `stop` still joins it.
//! - The thread blocks in `WaitForMultipleObjects` on the stop event, the
//!   wake event (a changed choice) and the streams' buffer events, with a
//!   100 ms timeout (loopback events fire only while audio renders), and
//!   checks the devices once a second.

use std::cell::Cell;
use std::fmt;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, E_ACCESSDENIED, E_INVALIDARG, HANDLE, WAIT_EVENT, WAIT_OBJECT_0,
};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_STREAMOPTIONS_NONE, AudioCategory_Speech,
    AudioClientProperties, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_E_UNSUPPORTED_FORMAT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, EDataFlow, IAcousticEchoCancellationControl, IAudioCaptureClient, IAudioClient, IAudioClient2,
    IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
use windows::core::{HRESULT, HSTRING, Interface, PCWSTR};

use super::devices;
use crate::eq_audio::{SampleFormat, mix_to_mono, read_format};

/// 100 ns units per second: the unit of every `qpc` in this module.
pub const QPC_UNITS_PER_SEC: u64 = 10_000_000;

/// A failed source is reopened on the current default for this long before
/// it is reported lost.
pub const RETRY_FOR: Duration = Duration::from_secs(2);
/// How often a failed source tries to reopen.
const RETRY_EVERY: Duration = Duration::from_millis(250);
/// The wait's timeout: loopback events fire only while something renders.
const WAKE_MS: u32 = 100;
/// How often the default endpoints are compared with the open ones.
const DEVICE_EVERY: Duration = Duration::from_secs(1);
/// The shared-mode buffer, 100 ms in 100 ns units.
const BUFFER_HNS: i64 = 1_000_000;
/// Packets taken per wake at most (a 100 ms buffer holds about ten).
const MAX_PACKETS: usize = 64;
/// `HRESULT_FROM_WIN32(ERROR_NOT_FOUND)`: no default endpoint of that kind.
const E_NOTFOUND: HRESULT = HRESULT(0x8007_0490_u32 as i32);

/// One audio source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A capture endpoint (`eCapture`); the default is the `eConsole` one.
    Mic,
    /// A render endpoint (`eRender`) in loopback; the default is the
    /// `eConsole` one.
    System,
}

/// Which endpoint each source uses, `[mic, system]`: an endpoint id, or
/// `None` to follow the Windows default. Shared between the caller (through
/// [`Capture::set_device`]) and the capture thread.
pub type Choices = Arc<Mutex<ChoiceSet>>;

/// The choices, with a generation per source that [`ChoiceSet::set`] raises, so
/// that choosing the same device again still counts as a new choice (it ends a
/// fall back to the default).
#[derive(Default)]
pub struct ChoiceSet {
    ids: [Option<String>; 2],
    generation: [u64; 2],
}

impl ChoiceSet {
    /// The choices `ids`, `[mic, system]`.
    pub fn new(ids: [Option<String>; 2]) -> ChoiceSet {
        ChoiceSet {
            ids,
            generation: [0; 2],
        }
    }

    /// A new choice for `source`, even when it equals the old one.
    pub fn set(&mut self, source: Source, id: Option<String>) {
        let i = source.index();
        self.ids[i] = id;
        self.generation[i] += 1;
    }
}

/// The names of the devices the sources stream from now, `[mic, system]`
/// (empty for a source that is not open).
type Names = Arc<Mutex<[String; 2]>>;

/// The sources in `Source::index` order.
const SOURCES: [Source; 2] = [Source::Mic, Source::System];

impl Source {
    pub(super) fn flow(self) -> EDataFlow {
        match self {
            Source::Mic => eCapture,
            Source::System => eRender,
        }
    }

    /// The place of the source in the `[mic, system]` arrays.
    pub fn index(self) -> usize {
        match self {
            Source::Mic => 0,
            Source::System => 1,
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::Mic => "microphone",
            Source::System => "system audio",
        })
    }
}

/// Which sources to capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sources {
    pub mic: bool,
    pub system: bool,
}

impl Sources {
    pub const BOTH: Sources = Sources {
        mic: true,
        system: true,
    };

    /// At least one source is on.
    pub fn any(self) -> bool {
        self.mic || self.system
    }

    /// The sources that are on, mic first.
    pub fn list(self) -> Vec<Source> {
        let mut out = Vec::with_capacity(2);
        if self.mic {
            out.push(Source::Mic);
        }
        if self.system {
            out.push(Source::System);
        }
        out
    }
}

impl Default for Sources {
    fn default() -> Self {
        Self::BOTH
    }
}

/// Why a source could not be captured.
#[derive(Debug)]
pub enum CaptureError {
    /// The microphone is refused by Windows (Settings, Privacy, Microphone).
    MicDenied,
    /// There is no default device of that kind.
    NoDevice(Source),
    /// The device went away and no default device could replace it.
    DeviceLost(Source),
    /// Any other failure, including a mix format that cannot be read
    /// (`AUDCLNT_E_UNSUPPORTED_FORMAT`).
    Com(windows::core::Error),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MicDenied => {
                f.write_str("microphone access is off in the Windows privacy settings")
            }
            Self::NoDevice(s) => write!(f, "no {s} device"),
            Self::DeviceLost(s) => write!(f, "the {s} device was lost"),
            Self::Com(e) => write!(f, "audio capture failed: {e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Receives the captured audio, on the capture thread.
///
/// The sink is dropped on the capture thread before that thread calls
/// `CoUninitialize`, so a sink may own COM objects (released in its `Drop`)
/// without outliving COM on the thread that releases them.
pub trait AudioSink: Send {
    /// One packet of `source`: mono samples at `rate` Hz whose first frame
    /// was captured at `qpc` (100 ns units, see [`qpc_now`]).
    fn audio(&mut self, source: Source, qpc: u64, rate: u32, mono: &[f32]);
    /// `source` stopped for good (it is not retried any more).
    fn lost(&mut self, source: Source, err: CaptureError);
    /// `source` moved to another device on its own: the Windows default
    /// changed (`fell_back` is false), or the device the user chose was
    /// missing or went away and the default took over (`fell_back` is true).
    /// `name` is the friendly name of the device now in use. A switch the
    /// caller asked for with [`Capture::set_device`] is reported too, with
    /// `manual` true (and `fell_back` false), so the caller can re-read the
    /// names without a notice; a chosen device that was missing is still a
    /// fall back.
    fn switched(&mut self, _source: Source, _name: &str, _fell_back: bool, _manual: bool) {}
}

/// The performance counter now, in the 100 ns units `AudioSink::audio` uses.
/// It is `QueryPerformanceCounter * 10_000_000 / QueryPerformanceFrequency`
/// (i128 maths), the same conversion `GetBuffer` applies to its
/// `pu64QPCPosition`, so the two compare directly.
pub fn qpc_now() -> u64 {
    qpc_hns() as u64
}

/// The performance counter now in 100 ns units, as an `i64`: the unit of the
/// packet times (`AudioSink::audio`'s `qpc`), for callers that keep times
/// signed (the screen recorder's audio input).
// Track C's screen recorder is the first caller outside this module.
#[allow(dead_code)]
pub fn qpc_hns() -> i64 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryPerformanceCounter(count: *mut i64) -> i32;
        fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
    }
    let (mut count, mut freq) = (0i64, 0i64);
    // SAFETY: both write one i64 through a valid pointer; they cannot fail on
    // Windows XP and later.
    unsafe {
        QueryPerformanceCounter(&mut count);
        QueryPerformanceFrequency(&mut freq);
    }
    hns_from_ticks(count, freq)
}

/// Raw performance-counter ticks at `freq` ticks per second to 100 ns units.
fn hns_from_ticks(ticks: i64, freq: i64) -> i64 {
    let freq = i128::from(freq.max(1));
    (i128::from(ticks.max(0)) * i128::from(QPC_UNITS_PER_SEC) / freq) as i64
}

/// A running capture. Dropping it stops it like [`Capture::stop`].
pub struct Capture {
    stop: Arc<Event>,
    /// Signalled when a choice changed.
    wake: Arc<Event>,
    choices: Choices,
    names: Names,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Opens `sources` on the devices in `choices` (the default where a
    /// choice is `None`) and starts delivering packets to `sink`. Fails only
    /// when none of them could be opened (with `MicDenied` if the mic was
    /// refused); a source that failed while another works is reported
    /// through `sink.lost` on the capture thread, and a chosen device that
    /// was missing through `sink.switched`. With `echo_cancel` every open of
    /// the mic asks Windows for echo cancellation (see [`Aec`]).
    pub fn start(
        sources: Sources,
        choices: Choices,
        echo_cancel: bool,
        sink: Box<dyn AudioSink>,
    ) -> Result<Capture, CaptureError> {
        if !sources.any() {
            return Err(CaptureError::Com(E_INVALIDARG.into()));
        }
        let stop = Arc::new(Event::new(true).map_err(CaptureError::Com)?);
        let wake = Arc::new(Event::new(false).map_err(CaptureError::Com)?);
        let names = Names::default();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let devs = Devices::new(
            Arc::clone(&choices),
            Arc::clone(&names),
            Arc::clone(&wake),
            echo_cancel,
            sources.system,
        );
        let worker_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("rec-capture".into())
            .spawn(move || run(sources, devs, sink, &worker_stop, &ready_tx))
            .map_err(|e| {
                CaptureError::Com(windows::core::Error::new(E_INVALIDARG, e.to_string()))
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Capture {
                stop,
                wake,
                choices,
                names,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(CaptureError::Com(windows::core::Error::new(
                    E_INVALIDARG,
                    "the capture thread ended before it was ready",
                )))
            }
        }
    }

    /// Points `source` at the endpoint `id`, or back at the Windows default
    /// (`None`), and wakes the capture thread so it switches at once. Never
    /// waits. A source that is not captured only remembers the choice.
    pub fn set_device(&self, source: Source, id: Option<String>) {
        self.choices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set(source, id);
        self.wake.set();
    }

    /// The friendly names of the devices the sources stream from now,
    /// `[mic, system]`; empty for a source that is not open.
    pub fn device_names(&self) -> [String; 2] {
        self.names.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Stops the capture: the buffered packets are still delivered, then the
    /// devices are released and the thread has ended when this returns.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.stop.set();
            let _ = thread.join();
        }
    }
}

/// An owned Win32 event.
struct Event(HANDLE);

// SAFETY: an event handle may be used from any thread.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    fn new(manual_reset: bool) -> windows::core::Result<Self> {
        unsafe { CreateEventW(None, manual_reset, false, None) }.map(Event)
    }

    fn set(&self) {
        unsafe {
            let _ = SetEvent(self.0);
        }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// What one source is doing.
enum Slot {
    /// Not requested, or lost for good.
    Off,
    Live(Stream),
    /// Failed; reopened every `RETRY_EVERY` until `until`, then reported.
    Retry {
        until: Instant,
        next: Instant,
        err: CaptureError,
    },
}

/// What the capture thread keeps about the devices.
struct Devices {
    choices: Choices,
    names: Names,
    /// Signalled by `Capture::set_device`.
    wake: Arc<Event>,
    /// The choices as the thread last read them, to see a change.
    applied: [Option<String>; 2],
    /// The generations of `applied`: a raised one is a new choice even when
    /// the id is the same.
    applied_generation: [u64; 2],
    /// A chosen device was missing or went away: the Windows default serves
    /// that source for the rest of the recording, until it is given a new
    /// choice.
    fallen: [bool; 2],
    /// The endpoint each source last streamed from, to tell a switch from a
    /// reopen of the same device.
    last: [Option<String>; 2],
    /// Mic echo cancellation is wanted for this recording.
    echo_cancel: bool,
    /// The PC audio is recorded.
    system_on: bool,
    /// The echo cancellation outcome was logged (once per recording).
    echo_logged: Cell<bool>,
    /// Raised whenever what the echo reference should be may have changed
    /// (the PC audio stream opened, a fall back, a new choice).
    epoch: u32,
    /// The `epoch` the mic stream was last pointed for.
    echo_epoch: u32,
}

/// The render endpoint the mic's echo canceller works against: the id of the
/// PC-audio stream that is open when the PC audio is recorded from a chosen
/// device (before it opens, the choice), otherwise `None`, which is Windows'
/// default render device.
fn echo_render(system_on: bool, chosen: Option<&str>, open: Option<&str>) -> Option<String> {
    if !system_on {
        return None;
    }
    let chosen = chosen?;
    Some(open.unwrap_or(chosen).to_owned())
}

impl Devices {
    fn new(
        choices: Choices,
        names: Names,
        wake: Arc<Event>,
        echo_cancel: bool,
        system_on: bool,
    ) -> Devices {
        let (applied, applied_generation) = {
            let set = choices.lock().unwrap_or_else(|e| e.into_inner());
            (set.ids.clone(), set.generation)
        };
        Devices {
            choices,
            names,
            wake,
            applied,
            applied_generation,
            fallen: [false; 2],
            last: [None, None],
            echo_cancel,
            system_on,
            echo_logged: Cell::new(false),
            epoch: 0,
            echo_epoch: 0,
        }
    }

    /// The echo reference now, see [`echo_render`].
    fn render_now(&self) -> Option<String> {
        let system = Source::System.index();
        echo_render(
            self.system_on,
            self.wanted(Source::System).as_deref(),
            self.last[system].as_deref(),
        )
    }

    /// How to open `source` now: the echo cancellation settings for the mic,
    /// pointed at the render endpoint the PC audio records.
    fn aec(&self, source: Source) -> Aec<'_> {
        Aec {
            on: self.echo_cancel && source == Source::Mic,
            render: self.render_now(),
            logged: &self.echo_logged,
        }
    }

    /// Reads the shared choices: which sources were given a new one since
    /// the last call (a new generation counts even for the same id). A new
    /// choice ends a fall back.
    fn refresh(&mut self) -> [bool; 2] {
        let (now, generation) = {
            let set = self.choices.lock().unwrap_or_else(|e| e.into_inner());
            (set.ids.clone(), set.generation)
        };
        let mut changed = [false; 2];
        for i in 0..2 {
            if now[i] != self.applied[i] || generation[i] != self.applied_generation[i] {
                changed[i] = true;
                self.fallen[i] = false;
            }
        }
        if changed[Source::System.index()] {
            self.epoch += 1;
        }
        self.applied = now;
        self.applied_generation = generation;
        changed
    }

    /// The choice to open `source` by: none (the default) after a fall back.
    fn wanted(&self, source: Source) -> Option<String> {
        let i = source.index();
        if self.fallen[i] {
            None
        } else {
            self.applied[i].clone()
        }
    }

    /// Notes that `stream` was opened: its name goes where the caller reads
    /// it. Returns what to report to the sink: a fall back always, any move
    /// to another device (flagged `manual` when the caller asked for it).
    fn opened(&mut self, stream: &Stream, fell_back: bool, manual: bool) -> Option<Switch> {
        let i = stream.source.index();
        self.names.lock().unwrap_or_else(|e| e.into_inner())[i] = stream.name.clone();
        let moved = self.last[i].as_deref().is_some_and(|l| l != stream.id);
        self.last[i] = Some(stream.id.clone());
        if i == Source::System.index() {
            self.epoch += 1;
        }
        if fell_back {
            self.fallen[i] = true;
            Some(Switch {
                fell_back: true,
                manual: false,
            })
        } else if moved {
            Some(Switch {
                fell_back: false,
                manual,
            })
        } else {
            None
        }
    }
}

/// What [`AudioSink::switched`] is told about a move to another device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Switch {
    fell_back: bool,
    manual: bool,
}

/// The capture thread.
fn run(
    sources: Sources,
    devs: Devices,
    sink: Box<dyn AudioSink>,
    stop: &Event,
    ready: &mpsc::SyncSender<Result<(), CaptureError>>,
) {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if let Err(e) = com.ok() {
        let _ = ready.send(Err(CaptureError::Com(e)));
        return;
    }
    // Takes the sink: it and every COM object are released in there, before
    // `CoUninitialize`.
    open_and_serve(sources, devs, sink, stop, ready);
    unsafe { CoUninitialize() };
}

/// The capture thread's work with COM initialised. Everything it owns,
/// the sink included, is dropped when it returns.
fn open_and_serve(
    sources: Sources,
    mut devs: Devices,
    mut sink: Box<dyn AudioSink>,
    stop: &Event,
    ready: &mpsc::SyncSender<Result<(), CaptureError>>,
) {
    let enumerator: windows::core::Result<IMMDeviceEnumerator> =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) };
    let enumerator = match enumerator {
        Ok(e) => e,
        Err(e) => {
            let _ = ready.send(Err(CaptureError::Com(e)));
            return;
        }
    };
    let mut slots = [Slot::Off, Slot::Off];
    let mut failed = Vec::new();
    let mut fell_back = Vec::new();
    for source in sources.list() {
        match open_wanted(&enumerator, source, devs.wanted(source).as_deref(), &devs.aec(source)) {
            Ok((s, fell)) => {
                if devs.opened(&s, fell, true).is_some_and(|w| w.fell_back) {
                    fell_back.push((source, s.name.clone()));
                }
                slots[source.index()] = Slot::Live(s);
            }
            Err(e) => failed.push((source, e)),
        }
    }
    if all_off(&slots) {
        let _ = ready.send(Err(pick_error(failed)));
        return;
    }
    let _ = ready.send(Ok(()));
    for (source, err) in failed {
        sink.lost(source, err);
    }
    for (source, name) in fell_back {
        sink.switched(source, &name, true, false);
    }
    serve(&enumerator, &mut slots, sink.as_mut(), stop, &mut devs);
    // The streams go first, then the sink, then the enumerator.
    drop(slots);
    drop(sink);
}

/// No source is open or being retried: nothing is left to wait for.
fn all_off(slots: &[Slot]) -> bool {
    slots.iter().all(|s| matches!(s, Slot::Off))
}

/// The error `start` returns when nothing opened: a refused mic first, so the
/// app can point at the privacy setting.
fn pick_error(failed: Vec<(Source, CaptureError)>) -> CaptureError {
    let mut first = None;
    for (_, e) in failed {
        if matches!(e, CaptureError::MicDenied) {
            return e;
        }
        first.get_or_insert(e);
    }
    first.unwrap_or(CaptureError::Com(E_INVALIDARG.into()))
}

/// The endpoint `choose` picks, with its reason.
#[derive(Debug, PartialEq, Eq)]
struct Pick {
    id: String,
    /// The choice was not available, so this is the default instead.
    fell_back: bool,
}

/// Which endpoint to open: the one the user `wanted` when it is among the
/// `available` ones, else the Windows `default` (and then, if something was
/// wanted, it fell back). `None` when there is nothing to open.
fn choose(wanted: Option<&str>, default: Option<&str>, available: &[String]) -> Option<Pick> {
    let pick = |id: &str, fell_back| Pick {
        id: id.to_owned(),
        fell_back,
    };
    match wanted {
        Some(id) if available.iter().any(|a| a == id) => Some(pick(id, false)),
        Some(_) => default.map(|d| pick(d, true)),
        None => default.map(|d| pick(d, false)),
    }
}

/// What to open for `source` now: [`choose`] over the devices as they are.
/// The active endpoints are listed only when something is wanted.
fn plan(enumerator: &IMMDeviceEnumerator, source: Source, wanted: Option<&str>) -> Option<Pick> {
    let available = if wanted.is_some() {
        devices::ids_in(enumerator, source)
    } else {
        Vec::new()
    };
    choose(wanted, default_id(enumerator, source).as_deref(), &available)
}

/// Opens what `pick` says. A chosen device that is there but will not open
/// (in use exclusively, a broken driver) is given up for the default.
fn open_pick(
    enumerator: &IMMDeviceEnumerator,
    source: Source,
    pick: Pick,
    wanted: Option<&str>,
    aec: &Aec,
) -> Result<(Stream, bool), CaptureError> {
    match Stream::open(enumerator, source, &pick.id, aec) {
        Ok(s) => Ok((s, pick.fell_back)),
        Err(e) if wanted.is_some() && !pick.fell_back && !matches!(e, CaptureError::MicDenied) => {
            let default = default_id(enumerator, source).ok_or(e)?;
            Stream::open(enumerator, source, &default, aec).map(|s| (s, true))
        }
        Err(e) => Err(e),
    }
}

/// [`plan`] and [`open_pick`] in one; `NoDevice` when there is nothing to open.
fn open_wanted(
    enumerator: &IMMDeviceEnumerator,
    source: Source,
    wanted: Option<&str>,
    aec: &Aec,
) -> Result<(Stream, bool), CaptureError> {
    let pick = plan(enumerator, source, wanted).ok_or(CaptureError::NoDevice(source))?;
    open_pick(enumerator, source, pick, wanted, aec)
}

/// The wait loop: drains every live stream on each wake, follows the devices
/// (the default, or the choice), reopens failed streams and ends on the stop
/// event, or once every source is lost for good (the thread then waits no
/// more; the recorder finishes by itself).
fn serve(
    enumerator: &IMMDeviceEnumerator,
    slots: &mut [Slot; 2],
    sink: &mut dyn AudioSink,
    stop: &Event,
    devs: &mut Devices,
) {
    let mut last_check = Instant::now();
    loop {
        let mut handles = vec![stop.0, devs.wake.0];
        for slot in slots.iter() {
            if let Slot::Live(s) = slot {
                handles.push(s.event.0);
            }
        }
        let woke = unsafe { WaitForMultipleObjects(&handles, false, WAKE_MS) };
        let stopping = woke == WAIT_OBJECT_0;
        let nudged = woke == WAIT_EVENT(WAIT_OBJECT_0.0 + 1);
        for (i, slot) in slots.iter_mut().enumerate() {
            let source = SOURCES[i];
            if let Slot::Live(stream) = slot
                && let Err(e) = stream.drain(sink)
            {
                *slot = fail(source, classify(source, e), sink);
            }
        }
        if stopping {
            return;
        }
        let now = Instant::now();
        let check = nudged || now.duration_since(last_check) >= DEVICE_EVERY;
        if check {
            last_check = now;
        }
        let changed = if check { devs.refresh() } else { [false; 2] };
        for (i, slot) in slots.iter_mut().enumerate() {
            let source = SOURCES[i];
            match slot {
                Slot::Live(stream) if check => {
                    // A default switch does not invalidate the old device,
                    // so the ids are compared (as `eq_audio` does).
                    let wanted = devs.wanted(source);
                    if let Some(pick) = plan(enumerator, source, wanted.as_deref()) {
                        if pick.id != stream.id {
                            if let Ok((new, fell)) =
                                open_pick(enumerator, source, pick, wanted.as_deref(), &devs.aec(source))
                            {
                                let report = devs.opened(&new, fell, changed[i]);
                                let name = new.name.clone();
                                *slot = Slot::Live(new);
                                if let Some(w) = report {
                                    sink.switched(source, &name, w.fell_back, w.manual);
                                }
                            }
                        } else if pick.fell_back
                            && let Some(w) = devs.opened(stream, true, changed[i])
                        {
                            // The chosen device is gone and the default is
                            // already the one in use.
                            sink.switched(source, &stream.name, w.fell_back, w.manual);
                        }
                    }
                }
                Slot::Retry { until, next, .. } if changed[i] || now >= *next => {
                    let until = *until;
                    let wanted = devs.wanted(source);
                    match open_wanted(enumerator, source, wanted.as_deref(), &devs.aec(source)) {
                        Ok((new, fell)) => {
                            let report = devs.opened(&new, fell, changed[i]);
                            let name = new.name.clone();
                            *slot = Slot::Live(new);
                            if let Some(w) = report {
                                sink.switched(source, &name, w.fell_back, w.manual);
                            }
                        }
                        Err(CaptureError::MicDenied) => {
                            *slot = Slot::Off;
                            sink.lost(source, CaptureError::MicDenied);
                        }
                        Err(_) if now >= until => {
                            if let Slot::Retry { err, .. } = std::mem::replace(slot, Slot::Off) {
                                sink.lost(source, err);
                            }
                        }
                        Err(_) => {
                            if let Slot::Retry { next, .. } = slot {
                                *next = now + RETRY_EVERY;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        follow_echo(slots, devs);
        if all_off(slots) {
            return;
        }
    }
}

/// Points the mic's echo canceller at the render endpoint it should work
/// against now, after the PC audio stream or its choice changed.
fn follow_echo(slots: &mut [Slot; 2], devs: &mut Devices) {
    if devs.epoch == devs.echo_epoch {
        return;
    }
    devs.echo_epoch = devs.epoch;
    let render = devs.render_now();
    if let Slot::Live(mic) = &mut slots[Source::Mic.index()] {
        mic.point_echo(render.as_deref(), &devs.echo_logged);
    }
}

/// The slot after `source` failed with `err`: a refused mic is lost at once,
/// anything else is retried.
fn fail(source: Source, err: CaptureError, sink: &mut dyn AudioSink) -> Slot {
    if matches!(err, CaptureError::MicDenied) {
        sink.lost(source, err);
        return Slot::Off;
    }
    let now = Instant::now();
    let err = match err {
        CaptureError::Com(e) if e.code() == AUDCLNT_E_DEVICE_INVALIDATED => {
            CaptureError::DeviceLost(source)
        }
        other => other,
    };
    Slot::Retry {
        until: now + RETRY_FOR,
        next: now,
        err,
    }
}

/// Maps a failure of `source` to its `CaptureError`.
fn classify(source: Source, e: windows::core::Error) -> CaptureError {
    let code = e.code();
    if code == E_ACCESSDENIED && source == Source::Mic {
        CaptureError::MicDenied
    } else if code == E_NOTFOUND {
        CaptureError::NoDevice(source)
    } else if code == AUDCLNT_E_DEVICE_INVALIDATED {
        CaptureError::DeviceLost(source)
    } else {
        CaptureError::Com(e)
    }
}

/// The id of the current default endpoint for `source`.
fn default_id(enumerator: &IMMDeviceEnumerator, source: Source) -> Option<String> {
    unsafe {
        enumerator
            .GetDefaultAudioEndpoint(source.flow(), eConsole)
            .ok()
            .and_then(|d| devices::device_id(&d))
    }
}

/// What the mic stream asks of Windows for echo cancellation: whether it is
/// wanted for this source, the render endpoint to cancel (`None`: the default
/// one) and where the outcome is logged, once per recording.
struct Aec<'a> {
    on: bool,
    render: Option<String>,
    logged: &'a Cell<bool>,
}

impl Aec<'_> {
    /// Logs `echo` the first time only.
    fn log_once(&self, echo: &Echo) {
        if !self.logged.replace(true) {
            eprintln!("vale: mic echo cancellation: {echo}");
        }
    }
}

/// How each best effort call went on one mic stream.
#[derive(Default)]
struct Echo {
    category: Step,
    endpoint: Step,
}

/// The outcome of one echo cancellation call.
#[derive(Default)]
enum Step {
    #[default]
    Skipped,
    Done,
    Failed(HRESULT),
}

impl Step {
    fn of(r: windows::core::Result<()>) -> Step {
        match r {
            Ok(()) => Step::Done,
            Err(e) => Step::Failed(e.code()),
        }
    }
}

impl fmt::Display for Echo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let one = |s: &Step| match s {
            Step::Skipped => "skipped".to_owned(),
            Step::Done => "ok".to_owned(),
            Step::Failed(code) => format!("failed ({:#010x})", code.0),
        };
        write!(
            f,
            "speech category {}, render endpoint {}",
            one(&self.category),
            one(&self.endpoint)
        )
    }
}

/// Puts the stream in the speech category, whose processing includes the
/// echo cancellation of Windows (or of the driver). Not the communications
/// category: that one makes Windows turn other apps down ("communications
/// activity" ducking, 80 % by default), and the meeting audio being recorded
/// with them. Measured on 2026-11-08: both categories cut the speaker sound in
/// the mic by about 95 %. Before `Initialize`.
fn set_speech_category(client: &IAudioClient) -> Step {
    Step::of((|| unsafe {
        let props = AudioClientProperties {
            cbSize: std::mem::size_of::<AudioClientProperties>() as u32,
            bIsOffload: false.into(),
            eCategory: AudioCategory_Speech,
            Options: AUDCLNT_STREAMOPTIONS_NONE,
        };
        client.cast::<IAudioClient2>()?.SetClientProperties(&props)
    })())
}

/// Points the echo canceller at the render endpoint `render` (the PC audio
/// being recorded; `None`: the default one).
fn point_control(control: &IAcousticEchoCancellationControl, render: Option<&str>) -> Step {
    unsafe {
        Step::of(match render {
            Some(id) => control.SetEchoCancellationRenderEndpoint(&HSTRING::from(id)),
            None => control.SetEchoCancellationRenderEndpoint(PCWSTR::null()),
        })
    }
}

/// The echo cancellation control of an initialized mic stream and the render
/// endpoint it is pointed at. Only Windows 11 22H2 and later has the
/// interface; on older builds there is none.
struct EchoControl {
    control: IAcousticEchoCancellationControl,
    render: Option<String>,
}

/// One open shared-mode stream, event driven.
struct Stream {
    source: Source,
    id: String,
    /// The device's friendly name.
    name: String,
    client: IAudioClient,
    capture: IAudioCaptureClient,
    /// Kept to point the mic's echo canceller again when the PC audio moves.
    echo: Option<EchoControl>,
    fmt: SampleFormat,
    rate: u32,
    mono: Vec<f32>,
    /// Where the previous packet ended (100 ns), for a packet whose
    /// timestamp is flagged as wrong.
    next_qpc: Option<u64>,
    /// Last field: closed after the client that signals it is released.
    event: Event,
}

impl Stream {
    /// Opens the endpoint `id` of `source`'s kind. A driver that rejects the
    /// speech category (`Initialize` fails after it was set) gets a
    /// plain stream instead of losing the source.
    fn open(
        enumerator: &IMMDeviceEnumerator,
        source: Source,
        id: &str,
        aec: &Aec,
    ) -> Result<Stream, CaptureError> {
        let mut rejected = false;
        match Self::open_with(enumerator, source, id, aec, &mut rejected) {
            Err(e) if rejected => {
                if !aec.logged.replace(true) {
                    eprintln!(
                        "vale: mic echo cancellation: the driver rejected the speech category ({e}), falling back to a plain stream"
                    );
                }
                let plain = Aec {
                    on: false,
                    render: None,
                    logged: aec.logged,
                };
                Self::open_with(enumerator, source, id, &plain, &mut false)
            }
            other => other,
        }
    }

    /// One try of [`Stream::open`]; `rejected` is set when `Initialize` failed
    /// after the speech category was set.
    fn open_with(
        enumerator: &IMMDeviceEnumerator,
        source: Source,
        id: &str,
        aec: &Aec,
        rejected: &mut bool,
    ) -> Result<Stream, CaptureError> {
        let err = |e| classify(source, e);
        unsafe {
            let device = enumerator.GetDevice(&HSTRING::from(id)).map_err(err)?;
            let name = devices::friendly_name(&device).unwrap_or_else(|| id.to_owned());
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(err)?;
            let mut echo = aec.on.then(Echo::default);
            if let Some(echo) = echo.as_mut() {
                echo.category = set_speech_category(&client);
            }
            let wfx = client.GetMixFormat().map_err(err)?;
            let parsed = read_format(wfx);
            let mut flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
            if source == Source::System {
                flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
            }
            // Shared mode, the mix format, a 100 ms buffer.
            let init = parsed.map(|_| {
                client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER_HNS, 0, wfx, None)
            });
            CoTaskMemFree(Some(wfx.cast_const().cast()));
            let Some((fmt, rate)) = parsed else {
                return Err(CaptureError::Com(AUDCLNT_E_UNSUPPORTED_FORMAT.into()));
            };
            if let Err(e) = init.expect("parsed is Some") {
                *rejected = matches!(&echo, Some(Echo { category: Step::Done, .. }));
                return Err(err(e));
            }
            let mut control = None;
            if let Some(echo) = echo.as_mut() {
                match client.GetService::<IAcousticEchoCancellationControl>() {
                    Ok(c) => {
                        echo.endpoint = point_control(&c, aec.render.as_deref());
                        control = Some(EchoControl {
                            control: c,
                            render: aec.render.clone(),
                        });
                    }
                    Err(_) => echo.endpoint = Step::Skipped,
                }
                aec.log_once(echo);
            }
            let event = Event::new(false).map_err(CaptureError::Com)?;
            client.SetEventHandle(event.0).map_err(err)?;
            let capture: IAudioCaptureClient = client.GetService().map_err(err)?;
            client.Start().map_err(err)?;
            Ok(Stream {
                source,
                id: id.to_owned(),
                name,
                client,
                capture,
                echo: control,
                fmt,
                rate,
                mono: Vec::new(),
                next_qpc: None,
                event,
            })
        }
    }

    /// Points this mic stream's echo canceller at `render` (`None`: the
    /// default render device) when it is not there already. A stream with no
    /// echo control does nothing.
    fn point_echo(&mut self, render: Option<&str>, logged: &Cell<bool>) {
        let Some(echo) = self.echo.as_mut() else {
            return;
        };
        if echo.render.as_deref() == render {
            return;
        }
        let step = point_control(&echo.control, render);
        echo.render = render.map(str::to_owned);
        if let Step::Failed(code) = step
            && !logged.replace(true)
        {
            eprintln!("vale: mic echo cancellation: render endpoint failed ({:#010x})", code.0);
        }
    }

    /// Hands every waiting packet to the sink; a silent packet is zeros.
    fn drain(&mut self, sink: &mut dyn AudioSink) -> windows::core::Result<()> {
        for _ in 0..MAX_PACKETS {
            let mut mono = std::mem::take(&mut self.mono);
            mono.clear();
            let qpc;
            unsafe {
                if self.capture.GetNextPacketSize()? == 0 {
                    self.mono = mono;
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let (mut frames, mut flags, mut stamp) = (0u32, 0u32, 0u64);
                self.capture.GetBuffer(
                    &mut data,
                    &mut frames,
                    &mut flags,
                    None,
                    Some(&mut stamp),
                )?;
                let n = frames as usize;
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    mono.resize(n, 0.0);
                } else {
                    let bytes = std::slice::from_raw_parts(data, n * self.fmt.block_align);
                    mix_to_mono(bytes, &self.fmt, &mut mono);
                }
                self.capture.ReleaseBuffer(frames)?;
                let bad_stamp = flags & (AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32) != 0;
                qpc = packet_time(bad_stamp, stamp, self.next_qpc);
            }
            self.next_qpc = Some(qpc + frames_to_qpc(mono.len() as u64, self.rate));
            sink.audio(self.source, qpc, self.rate, &mono);
            self.mono = mono;
        }
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

/// The time of a packet: its own stamp, or (when the device flags the stamp
/// as wrong) where the previous packet ended, or now.
fn packet_time(bad_stamp: bool, stamp: u64, next: Option<u64>) -> u64 {
    match (bad_stamp, next) {
        (false, _) => stamp,
        (true, Some(next)) => next,
        (true, None) => qpc_now(),
    }
}

/// `frames` at `rate` in 100 ns units.
fn frames_to_qpc(frames: u64, rate: u32) -> u64 {
    (u128::from(frames) * u128::from(QPC_UNITS_PER_SEC) / u128::from(rate.max(1))) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both sources on the Windows default.
    fn follow() -> Choices {
        Choices::default()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn pick(id: &str, fell_back: bool) -> Option<Pick> {
        Some(Pick {
            id: id.to_owned(),
            fell_back,
        })
    }

    #[test]
    fn following_windows_opens_the_default() {
        assert_eq!(choose(None, Some("d"), &ids(&["a", "d"])), pick("d", false));
        // Even with an empty list: the default is the default.
        assert_eq!(choose(None, Some("d"), &[]), pick("d", false));
        assert_eq!(choose(None, None, &ids(&["a"])), None);
    }

    #[test]
    fn a_chosen_device_that_is_there_wins_over_the_default() {
        assert_eq!(choose(Some("a"), Some("d"), &ids(&["a", "d"])), pick("a", false));
        assert_eq!(choose(Some("a"), None, &ids(&["a"])), pick("a", false));
        assert_eq!(choose(Some("d"), Some("d"), &ids(&["d"])), pick("d", false));
    }

    #[test]
    fn a_missing_chosen_device_falls_back_to_the_default() {
        assert_eq!(choose(Some("gone"), Some("d"), &ids(&["d"])), pick("d", true));
        assert_eq!(choose(Some("gone"), Some("d"), &[]), pick("d", true));
        // Nothing to fall back to.
        assert_eq!(choose(Some("gone"), None, &ids(&["a"])), None);
    }

    #[test]
    fn the_echo_reference_is_the_open_pc_audio_stream_only_for_a_chosen_device() {
        // PC audio not recorded, or on the Windows default: Windows' default.
        assert_eq!(echo_render(false, Some("c"), Some("o")), None);
        assert_eq!(echo_render(true, None, Some("o")), None);
        assert_eq!(echo_render(true, None, None), None);
        // A chosen device: the open stream's id, before it opens the choice.
        assert_eq!(echo_render(true, Some("c"), Some("o")).as_deref(), Some("o"));
        assert_eq!(echo_render(true, Some("c"), None).as_deref(), Some("c"));
    }

    #[test]
    fn only_the_mic_gets_echo_cancellation_and_it_follows_the_pc_audio_endpoint() {
        let choices = follow();
        let wake = Arc::new(Event::new(false).expect("event"));
        let mut devs = Devices::new(Arc::clone(&choices), Names::default(), wake, true, true);
        assert!(devs.aec(Source::Mic).on);
        assert!(!devs.aec(Source::System).on, "the loopback stream is never changed");
        // No choice: the default render endpoint (None), open stream or not.
        assert_eq!(devs.aec(Source::Mic).render, None);
        devs.last[1] = Some("open".into());
        assert_eq!(devs.aec(Source::Mic).render, None);
        // A choice is the endpoint; the open stream's id wins over it.
        devs.applied[1] = Some("chosen".into());
        assert_eq!(devs.aec(Source::Mic).render.as_deref(), Some("open"));
        devs.last[1] = None;
        assert_eq!(devs.aec(Source::Mic).render.as_deref(), Some("chosen"));
        // After a fall back the default serves the PC audio.
        devs.fallen[1] = true;
        assert_eq!(devs.aec(Source::Mic).render, None);
        devs.fallen[1] = false;
        devs.echo_cancel = false;
        assert!(!devs.aec(Source::Mic).on);
        // Without the PC audio there is no endpoint to name.
        devs.system_on = false;
        assert_eq!(devs.render_now(), None);
    }

    #[test]
    fn a_choice_is_set_from_another_thread_and_read_by_the_capture_thread() {
        let choices = follow();
        let wake = Arc::new(Event::new(false).expect("event"));
        let mut devs = Devices::new(Arc::clone(&choices), Names::default(), wake, true, true);
        assert_eq!(devs.refresh(), [false, false]);
        choices.lock().unwrap().set(Source::System, Some("x".into()));
        assert_eq!(devs.refresh(), [false, true]);
        assert_eq!(devs.wanted(Source::System).as_deref(), Some("x"));
        assert_eq!(devs.wanted(Source::Mic), None);
        // Read once: no change the second time.
        assert_eq!(devs.refresh(), [false, false]);
        // A fall back sets the choice aside until a new one comes.
        devs.fallen[1] = true;
        assert_eq!(devs.wanted(Source::System), None);
        choices.lock().unwrap().set(Source::System, Some("y".into()));
        assert_eq!(devs.refresh(), [false, true]);
        assert_eq!(devs.wanted(Source::System).as_deref(), Some("y"));
    }

    #[test]
    fn choosing_the_same_device_again_ends_a_fall_back() {
        let choices = follow();
        let wake = Arc::new(Event::new(false).expect("event"));
        choices.lock().unwrap().set(Source::System, Some("x".into()));
        let mut devs = Devices::new(Arc::clone(&choices), Names::default(), wake, true, true);
        // The chosen device went away: the default serves, `x` is set aside.
        devs.fallen[1] = true;
        assert_eq!(devs.wanted(Source::System), None);
        assert_eq!(devs.refresh(), [false, false]);
        assert!(devs.fallen[1], "nothing was chosen yet");
        // The user picks `x` again.
        choices.lock().unwrap().set(Source::System, Some("x".into()));
        assert_eq!(devs.refresh(), [false, true], "the same id is a new choice");
        assert!(!devs.fallen[1]);
        assert_eq!(devs.wanted(Source::System).as_deref(), Some("x"));
        assert_eq!(devs.refresh(), [false, false]);
    }

    #[test]
    fn sources_list_mic_first() {
        assert_eq!(Sources::BOTH.list(), vec![Source::Mic, Source::System]);
        assert_eq!(
            Sources {
                mic: false,
                system: true
            }
            .list(),
            vec![Source::System]
        );
        assert!(
            !Sources {
                mic: false,
                system: false
            }
            .any()
        );
        assert_eq!(Sources::default(), Sources::BOTH);
    }

    #[test]
    fn errors_are_classified() {
        let e = |h: HRESULT| windows::core::Error::from(h);
        assert!(matches!(
            classify(Source::Mic, e(E_ACCESSDENIED)),
            CaptureError::MicDenied
        ));
        // Access denied on the loopback is not a mic permission problem.
        assert!(matches!(
            classify(Source::System, e(E_ACCESSDENIED)),
            CaptureError::Com(_)
        ));
        assert!(matches!(
            classify(Source::System, e(E_NOTFOUND)),
            CaptureError::NoDevice(Source::System)
        ));
        assert!(matches!(
            classify(Source::Mic, e(AUDCLNT_E_DEVICE_INVALIDATED)),
            CaptureError::DeviceLost(Source::Mic)
        ));
        assert!(matches!(
            classify(Source::Mic, e(E_INVALIDARG)),
            CaptureError::Com(_)
        ));
    }

    #[test]
    fn a_refused_mic_wins_when_nothing_opened() {
        let failed = vec![
            (Source::System, CaptureError::NoDevice(Source::System)),
            (Source::Mic, CaptureError::MicDenied),
        ];
        assert!(matches!(pick_error(failed), CaptureError::MicDenied));
        let failed = vec![
            (Source::Mic, CaptureError::NoDevice(Source::Mic)),
            (Source::System, CaptureError::Com(E_INVALIDARG.into())),
        ];
        assert!(matches!(
            pick_error(failed),
            CaptureError::NoDevice(Source::Mic)
        ));
    }

    #[test]
    fn errors_read_well() {
        assert_eq!(
            CaptureError::NoDevice(Source::Mic).to_string(),
            "no microphone device"
        );
        assert_eq!(
            CaptureError::DeviceLost(Source::System).to_string(),
            "the system audio device was lost"
        );
    }

    #[test]
    fn a_wrong_stamp_continues_the_previous_packet() {
        assert_eq!(packet_time(false, 123, Some(9)), 123);
        assert_eq!(packet_time(true, 123, Some(9)), 9);
        let now = qpc_now();
        assert!(packet_time(true, 0, None) >= now);
        assert_eq!(frames_to_qpc(480, 48_000), 100_000);
        assert_eq!(frames_to_qpc(441, 44_100), 100_000);
    }

    #[test]
    fn the_clock_is_in_100ns_units_not_raw_ticks() {
        // GetBuffer's pu64QPCPosition is 10_000_000 * ticks / frequency.
        assert_eq!(hns_from_ticks(10_000_000, 10_000_000), 10_000_000);
        assert_eq!(hns_from_ticks(3_000_000, 3_000_000), 10_000_000); // 1 s at 3 MHz
        assert_eq!(hns_from_ticks(24_000_000, 24_000_000), 10_000_000); // 1 s at 24 MHz
        // Hours of ticks at 24 MHz times 10^7 overflow i64 but not i128.
        let day = 24_000_000i64 * 86_400;
        assert_eq!(hns_from_ticks(day, 24_000_000), 86_400 * 10_000_000);
        // Measured against `Instant`: 30 ms is about 300_000 units.
        let (a, t) = (qpc_hns(), std::time::Instant::now());
        std::thread::sleep(Duration::from_millis(30));
        let (b, real) = (qpc_hns(), t.elapsed());
        let diff_ms = (b - a) / 10_000;
        // The two clocks are read a moment apart; a preempted test thread
        // can stretch that moment.
        assert!(
            (diff_ms - real.as_millis() as i64).abs() <= 5,
            "{diff_ms} ms vs {real:?}"
        );
        assert!(qpc_now() >= b as u64);
    }

    /// Works with or without an output device: the sink goes on either path.
    #[test]
    fn the_sink_is_dropped_while_com_still_runs_on_the_capture_thread() {
        use std::sync::atomic::{AtomicU8, Ordering};
        struct Probe(Arc<AtomicU8>);
        impl AudioSink for Probe {
            fn audio(&mut self, _: Source, _: u64, _: u32, _: &[f32]) {}
            fn lost(&mut self, _: Source, _: CaptureError) {}
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                use windows::Win32::System::Com::{
                    APTTYPE, APTTYPEQUALIFIER, APTTYPEQUALIFIER_IMPLICIT_MTA, CoGetApartmentType,
                };
                let (mut t, mut q) = (APTTYPE(0), APTTYPEQUALIFIER(0));
                // Without its own `CoInitializeEx` a thread still reads as
                // the process's implicit MTA; that does not count.
                let com = unsafe { CoGetApartmentType(&mut t, &mut q) }.is_ok()
                    && q != APTTYPEQUALIFIER_IMPLICIT_MTA;
                self.0.store(if com { 1 } else { 2 }, Ordering::SeqCst);
            }
        }
        // A `rec-capture` thread: the recorder's sessions count those.
        let _one = super::super::tests::session();
        let seen = Arc::new(AtomicU8::new(0));
        let system = Sources {
            mic: false,
            system: true,
        };
        if let Ok(c) = Capture::start(system, follow(), true, Box::new(Probe(Arc::clone(&seen)))) {
            c.stop();
        }
        assert_eq!(seen.load(Ordering::SeqCst), 1, "1: COM was up, 2: it was not");
    }

    /// Reports what the capture says about devices, for the tests.
    struct Reports(std::sync::mpsc::Sender<(Source, String, bool, bool)>);

    impl AudioSink for Reports {
        fn audio(&mut self, _: Source, _: u64, _: u32, _: &[f32]) {}
        fn lost(&mut self, _: Source, _: CaptureError) {}
        fn switched(&mut self, source: Source, name: &str, fell_back: bool, manual: bool) {
            let _ = self.0.send((source, name.to_owned(), fell_back, manual));
        }
    }

    /// Needs an output device (and says nothing without one).
    #[test]
    fn a_missing_chosen_output_falls_back_to_the_default_and_a_hand_pick_is_silent() {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        let _one = super::super::tests::session();
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let Some(default) = devices::default_endpoint(Source::System) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let system = Sources {
            mic: false,
            system: true,
        };
        let choices = follow();
        choices
            .lock()
            .unwrap()
            .set(Source::System, Some("{0.0.0.00000000}.{no-such-device}".into()));
        let Ok(c) = Capture::start(system, Arc::clone(&choices), true, Box::new(Reports(tx))) else {
            return;
        };
        // At the start: the default stands in, and the choice is left alone.
        let got = rx.recv_timeout(Duration::from_secs(2)).expect("a report");
        assert_eq!(got, (Source::System, default.name.clone(), true, false));
        assert_eq!(c.device_names()[1], default.name);
        assert_eq!(c.device_names()[0], "", "the mic is not captured");
        assert_eq!(
            choices.lock().unwrap().ids[1].as_deref(),
            Some("{0.0.0.00000000}.{no-such-device}")
        );
        // Picking the default by hand switches nothing and says nothing.
        c.set_device(Source::System, Some(default.id.clone()));
        std::thread::sleep(Duration::from_millis(400));
        assert!(rx.try_recv().is_err(), "a pick of the user's is not reported");
        // Picking a device that is not there while on the default says so.
        c.set_device(Source::System, Some("{0.0.0.00000000}.{another-missing}".into()));
        let got = rx.recv_timeout(Duration::from_secs(2)).expect("a second report");
        assert_eq!(got, (Source::System, default.name.clone(), true, false));
        // Back to following Windows: silent again.
        c.set_device(Source::System, None);
        std::thread::sleep(Duration::from_millis(400));
        assert!(rx.try_recv().is_err());
        assert_eq!(c.device_names()[1], default.name);
        c.stop();
    }

    /// Needs two output devices that open (and says nothing with fewer).
    #[test]
    fn a_picked_output_takes_over_at_once_and_back_to_the_default_follows_windows() {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        let _one = super::super::tests::session();
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let Some(default) = devices::default_endpoint(Source::System) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let system = Sources {
            mic: false,
            system: true,
        };
        let Ok(c) = Capture::start(system, follow(), true, Box::new(Reports(tx))) else {
            return;
        };
        assert_eq!(c.device_names()[1], default.name);
        let others: Vec<_> = devices::list(Source::System)
            .into_iter()
            .filter(|e| e.id != default.id)
            .collect();
        for other in others {
            c.set_device(Source::System, Some(other.id.clone()));
            let until = Instant::now() + Duration::from_secs(2);
            while c.device_names()[1] != other.name && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            // Opens on it, or (a device that will not open) stays on the default.
            let now = c.device_names()[1].clone();
            assert!(now == other.name || now == default.name, "{now}");
            if now == other.name {
                c.set_device(Source::System, None);
                let until = Instant::now() + Duration::from_secs(2);
                while c.device_names()[1] != default.name && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(20));
                }
                assert_eq!(c.device_names()[1], default.name);
                // The hand picks are reported as such, never as a fall back.
                assert!(rx.try_iter().all(|r| r.3 && !r.2), "hand picks are manual");
                break;
            }
            // A fall back was reported for the one that would not open.
            assert!(rx.try_recv().is_ok());
        }
        c.stop();
    }

    #[test]
    fn starting_with_no_source_fails_without_a_thread() {
        struct Nothing;
        impl AudioSink for Nothing {
            fn audio(&mut self, _: Source, _: u64, _: u32, _: &[f32]) {}
            fn lost(&mut self, _: Source, _: CaptureError) {}
        }
        let none = Sources {
            mic: false,
            system: false,
        };
        assert!(Capture::start(none, follow(), true, Box::new(Nothing)).is_err());
    }
}
