//! Audio capture for recordings: the default microphone, the system audio
//! (WASAPI loopback on the default output) or both, delivered as mono `f32`
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
//! - A source whose default device changes is reopened on the new default
//!   without a report. A source that fails (the device is unplugged,
//!   `AUDCLNT_E_DEVICE_INVALIDATED`, or the audio service restarts) is
//!   reopened on the current default for up to [`RETRY_FOR`]; when that does
//!   not work, [`AudioSink::lost`] is called once and that source stays off.
//!   The other source keeps running. A microphone refused by the Windows
//!   privacy setting is [`CaptureError::MicDenied`], never retried.
//! - [`Capture::stop`] (or dropping the `Capture`) signals the thread, which
//!   delivers the packets still buffered, releases the devices and ends;
//!   `stop` returns after the thread has ended. No thread or timer exists
//!   before `start` or after `stop`. When every source is lost for good the
//!   thread ends by itself (no more 100 ms wakes); `stop` still joins it.
//! - The thread blocks in `WaitForMultipleObjects` on the stop event and the
//!   streams' buffer events, with a 100 ms timeout (loopback events fire only
//!   while audio renders), and checks the default devices once a second.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, E_ACCESSDENIED, E_INVALIDARG, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_E_UNSUPPORTED_FORMAT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, EDataFlow, IAudioCaptureClient, IAudioClient, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
use windows::core::HRESULT;

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
    /// The default capture endpoint (`eCapture`, `eConsole`).
    Mic,
    /// The default render endpoint (`eRender`, `eConsole`) in loopback.
    System,
}

impl Source {
    fn flow(self) -> EDataFlow {
        match self {
            Source::Mic => eCapture,
            Source::System => eRender,
        }
    }

    fn index(self) -> usize {
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
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Opens `sources` and starts delivering packets to `sink`. Fails only
    /// when none of them could be opened (with `MicDenied` if the mic was
    /// refused); a source that failed while another works is reported
    /// through `sink.lost` on the capture thread.
    pub fn start(sources: Sources, sink: Box<dyn AudioSink>) -> Result<Capture, CaptureError> {
        if !sources.any() {
            return Err(CaptureError::Com(E_INVALIDARG.into()));
        }
        let stop = Arc::new(Event::new(true).map_err(CaptureError::Com)?);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("rec-capture".into())
            .spawn(move || run(sources, sink, &worker_stop, &ready_tx))
            .map_err(|e| {
                CaptureError::Com(windows::core::Error::new(E_INVALIDARG, e.to_string()))
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Capture {
                stop,
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

/// The capture thread.
fn run(
    sources: Sources,
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
    open_and_serve(sources, sink, stop, ready);
    unsafe { CoUninitialize() };
}

/// The capture thread's work with COM initialised. Everything it owns,
/// the sink included, is dropped when it returns.
fn open_and_serve(
    sources: Sources,
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
    for source in sources.list() {
        match Stream::open(&enumerator, source) {
            Ok(s) => slots[source.index()] = Slot::Live(s),
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
    serve(&enumerator, &mut slots, sink.as_mut(), stop);
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

/// The wait loop: drains every live stream on each wake, follows the default
/// devices, reopens failed streams and ends on the stop event, or once every
/// source is lost for good (the thread then waits no more; the recorder
/// finishes by itself).
fn serve(
    enumerator: &IMMDeviceEnumerator,
    slots: &mut [Slot; 2],
    sink: &mut dyn AudioSink,
    stop: &Event,
) {
    let mut last_check = Instant::now();
    loop {
        let mut handles = vec![stop.0];
        for slot in slots.iter() {
            if let Slot::Live(s) = slot {
                handles.push(s.event.0);
            }
        }
        let woke = unsafe { WaitForMultipleObjects(&handles, false, WAKE_MS) };
        let stopping = woke == WAIT_OBJECT_0;
        for (i, slot) in slots.iter_mut().enumerate() {
            let source = if i == 0 { Source::Mic } else { Source::System };
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
        let check = now.duration_since(last_check) >= DEVICE_EVERY;
        if check {
            last_check = now;
        }
        for (i, slot) in slots.iter_mut().enumerate() {
            let source = if i == 0 { Source::Mic } else { Source::System };
            match slot {
                Slot::Live(stream) if check => {
                    // A default switch does not invalidate the old device,
                    // so the ids are compared (as `eq_audio` does).
                    let current = default_id(enumerator, source);
                    if current.is_some()
                        && current.as_deref() != Some(stream.id.as_str())
                        && let Ok(new) = Stream::open(enumerator, source)
                    {
                        *slot = Slot::Live(new);
                    }
                }
                Slot::Retry { until, next, .. } if now >= *next => {
                    let until = *until;
                    match Stream::open(enumerator, source) {
                        Ok(new) => *slot = Slot::Live(new),
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
        if all_off(slots) {
            return;
        }
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
            .and_then(|d| device_id(&d))
    }
}

fn device_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let raw = device.GetId().ok()?;
        let id = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0.cast_const().cast()));
        id
    }
}

/// One open shared-mode stream, event driven.
struct Stream {
    source: Source,
    id: String,
    client: IAudioClient,
    capture: IAudioCaptureClient,
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
    fn open(enumerator: &IMMDeviceEnumerator, source: Source) -> Result<Stream, CaptureError> {
        let err = |e| classify(source, e);
        unsafe {
            let device = enumerator
                .GetDefaultAudioEndpoint(source.flow(), eConsole)
                .map_err(err)?;
            let id = device_id(&device).unwrap_or_default();
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(err)?;
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
            init.expect("parsed is Some").map_err(err)?;
            let event = Event::new(false).map_err(CaptureError::Com)?;
            client.SetEventHandle(event.0).map_err(err)?;
            let capture: IAudioCaptureClient = client.GetService().map_err(err)?;
            client.Start().map_err(err)?;
            Ok(Stream {
                source,
                id,
                client,
                capture,
                fmt,
                rate,
                mono: Vec::new(),
                next_qpc: None,
                event,
            })
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
        if let Ok(c) = Capture::start(system, Box::new(Probe(Arc::clone(&seen)))) {
            c.stop();
        }
        assert_eq!(seen.load(Ordering::SeqCst), 1, "1: COM was up, 2: it was not");
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
        assert!(Capture::start(none, Box::new(Nothing)).is_err());
    }
}
