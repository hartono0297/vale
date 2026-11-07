//! The compact view's equalizer bars follow the real music: the default
//! output device is captured in loopback (what the speakers play), split
//! into five frequency bands and turned into five 0..=1 levels.
//!
//! The top half is pure and tested (sample formats, the mono mix-down, the
//! band-pass filters and the `Analyzer` that turns band energy into smooth
//! levels). The bottom half is the `eq-capture` worker thread that owns COM
//! and the WASAPI client; it runs only while the UI asks for it
//! (`EqCapture::set_running`) and blocks otherwise. The UI reads the levels
//! from `EqShared` without ever waiting on the worker, and the worker never
//! wakes or posts to the UI: the existing equalizer timer is the only repaint.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eMultimedia, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::core::GUID;

/// How many bars (and bands) there are.
pub const BANDS: usize = 5;

/// The band edges in Hz, low to high: bass, low mids, mids, upper mids, highs.
pub const EDGES: [(f32, f32); BANDS] = [
    (60.0, 150.0),
    (150.0, 400.0),
    (400.0, 1000.0),
    (1000.0, 3000.0),
    (3000.0, 8000.0),
];

/// A band's level is one RMS per hop of this many seconds of samples.
pub const HOP_SECS: f32 = 0.010;
/// The slow auto-gain: a band's remembered peak falls with this time constant.
pub const AGC_TAU: f32 = 3.0;
/// The auto-gain never divides by less than this, so a faint band stays low.
pub const FLOOR: f32 = 0.003;
/// A band quieter than this (RMS, full scale 1.0) is silence.
pub const GATE: f32 = 0.0003;
/// The shape applied to a normalised level: below 1, so quiet parts stay low.
pub const SHAPE: f32 = 1.5;
/// Time constants of the smoothing, in seconds: quick to rise, slow to fall.
pub const ATTACK_TAU: f32 = 0.030;
pub const DECAY_TAU: f32 = 0.200;
/// Hops kept between two `advance` calls (1.28 s); older ones are dropped.
const MAX_PENDING: usize = 128;

// ---- sample formats (pure) ---------------------------------------------

/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`; `Win32_Media_Multimedia` is not enabled,
/// so the format ids are written here.
const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
/// `KSDATAFORMAT_SUBTYPE_PCM`.
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const TAG_PCM: u16 = 1;
const TAG_FLOAT: u16 = 3;
const TAG_EXTENSIBLE: u16 = 0xFFFE;

/// How one sample is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample {
    F32,
    I16,
    /// 24 bit, packed in 3 bytes.
    I24,
    /// 32 bit integer container.
    I32,
}

impl Sample {
    fn bytes(self) -> usize {
        match self {
            Sample::F32 | Sample::I32 => 4,
            Sample::I16 => 2,
            Sample::I24 => 3,
        }
    }
}

/// A stream's interleaved layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleFormat {
    pub sample: Sample,
    pub channels: usize,
    /// Bytes per frame (all channels).
    pub block_align: usize,
}

/// The layout a `WAVEFORMATEX` (or `WAVEFORMATEXTENSIBLE` with `sub`) names;
/// `None` for anything the analysis does not read (the caller then falls back
/// to the sine bars).
pub fn parse_format(
    tag: u16,
    channels: u16,
    bits: u16,
    block_align: u16,
    sub: Option<GUID>,
) -> Option<SampleFormat> {
    let (float, int) = match tag {
        TAG_FLOAT => (true, false),
        TAG_PCM => (false, true),
        TAG_EXTENSIBLE => match sub? {
            s if s == SUBTYPE_FLOAT => (true, false),
            s if s == SUBTYPE_PCM => (false, true),
            _ => return None,
        },
        _ => return None,
    };
    let sample = match (float, int, bits) {
        (true, _, 32) => Sample::F32,
        (_, true, 16) => Sample::I16,
        (_, true, 24) => Sample::I24,
        (_, true, 32) => Sample::I32,
        _ => return None,
    };
    let channels = usize::from(channels);
    let block_align = usize::from(block_align);
    if channels == 0 || block_align < channels * sample.bytes() {
        return None;
    }
    Some(SampleFormat {
        sample,
        channels,
        block_align,
    })
}

/// Appends the mono mix of every whole frame in `bytes` to `out`: the channels
/// averaged, each sample scaled to -1..=1 (a sample that is not finite is 0).
pub fn mix_to_mono(bytes: &[u8], fmt: &SampleFormat, out: &mut Vec<f32>) {
    let width = fmt.sample.bytes();
    let frames = bytes.len() / fmt.block_align.max(1);
    out.reserve(frames);
    for frame in bytes.chunks_exact(fmt.block_align).take(frames) {
        let mut sum = 0.0f32;
        for ch in 0..fmt.channels {
            let b = &frame[ch * width..(ch + 1) * width];
            let v = match fmt.sample {
                Sample::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                Sample::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
                Sample::I24 => {
                    // Sign-extend the top byte into a 32 bit value.
                    let raw = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
                    raw as f32 / 8_388_608.0
                }
                Sample::I32 => {
                    i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0
                }
            };
            sum += if v.is_finite() { v } else { 0.0 };
        }
        out.push(sum / fmt.channels as f32);
    }
}

// ---- the band-pass filters (pure) --------------------------------------

/// A band's centre in Hz: the geometric mean of its edges, kept below
/// `0.45 * sample_rate` so a low sample rate still gets a stable filter.
pub fn band_centre(band: usize, sample_rate: f32) -> f32 {
    let (lo, hi) = EDGES[band];
    (lo * hi).sqrt().min(0.45 * sample_rate)
}

/// An RBJ band-pass biquad with constant 0 dB peak gain (transposed form II,
/// in f64: the 60 Hz band at 48 kHz has poles very near the unit circle).
#[derive(Clone, Debug)]
struct Biquad {
    b0: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn band(band: usize, sample_rate: f32) -> Self {
        let (lo, hi) = EDGES[band];
        let centre = f64::from(band_centre(band, sample_rate));
        let q = centre / f64::from(hi - lo);
        let w0 = std::f64::consts::TAU * centre / f64::from(sample_rate);
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        // b1 = 0 and b2 = -b0 for the constant-peak band-pass.
        let y = self.b0 * x + self.z1;
        self.z1 = self.z2 - self.a1 * y;
        self.z2 = -self.b0 * x - self.a2 * y;
        y
    }
}

// ---- the analyzer (pure) ------------------------------------------------

/// Mono samples in, five smooth 0..=1 levels out.
///
/// `push` filters the samples into the five bands and closes a hop (the RMS
/// of each band over 10 ms of samples) whenever one is full. `advance` is
/// called once per UI tick: it spreads the tick's `dt` over the hops that
/// arrived (or runs one step towards zero when none did, which is what
/// loopback gives in silence), applies the auto-gain, the gate and the shape,
/// and smooths with a fast attack and a slow decay.
#[derive(Clone, Debug)]
pub struct Analyzer {
    filters: [Biquad; BANDS],
    hop: usize,
    filled: usize,
    sum_sq: [f64; BANDS],
    pending: Vec<[f32; BANDS]>,
    peak: [f32; BANDS],
    level: [f32; BANDS],
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let rate = if sample_rate == 0 {
            48_000.0
        } else {
            sample_rate as f32
        };
        Self {
            filters: std::array::from_fn(|i| Biquad::band(i, rate)),
            hop: ((rate * HOP_SECS) as usize).max(1),
            filled: 0,
            sum_sq: [0.0; BANDS],
            pending: Vec::new(),
            peak: [0.0; BANDS],
            level: [0.0; BANDS],
        }
    }

    /// Feeds mono samples (-1..=1).
    pub fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            let x = if s.is_finite() { f64::from(s) } else { 0.0 };
            for (filter, sum) in self.filters.iter_mut().zip(self.sum_sq.iter_mut()) {
                let y = filter.process(x);
                *sum += y * y;
            }
            self.filled += 1;
            if self.filled == self.hop {
                let n = self.hop as f64;
                let rms = std::array::from_fn(|i| (self.sum_sq[i] / n).sqrt() as f32);
                if self.pending.len() == MAX_PENDING {
                    self.pending.remove(0);
                }
                self.pending.push(rms);
                self.sum_sq = [0.0; BANDS];
                self.filled = 0;
            }
        }
    }

    /// Moves the levels on by `dt` seconds and returns them. `muted` pulls
    /// every target to 0 (the output is silent to the listener, though
    /// loopback still hears the stream), and the bars then decay to rest.
    pub fn advance(&mut self, dt: f32, muted: bool) -> [f32; BANDS] {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let hops = std::mem::take(&mut self.pending);
        if hops.is_empty() {
            self.step([0.0; BANDS], dt, muted);
        } else {
            let each = dt / hops.len() as f32;
            for rms in hops {
                self.step(rms, each, muted);
            }
        }
        self.level
    }

    /// The levels as of the last `advance`.
    #[cfg(test)]
    fn levels(&self) -> [f32; BANDS] {
        self.level
    }

    fn step(&mut self, rms: [f32; BANDS], dt: f32, muted: bool) {
        let fall = (-dt / AGC_TAU).exp();
        for i in 0..BANDS {
            let r = if rms[i] < GATE { 0.0 } else { rms[i] };
            self.peak[i] = r.max(self.peak[i] * fall);
            let norm = r / self.peak[i].max(FLOOR);
            let target = if muted {
                0.0
            } else {
                norm.clamp(0.0, 1.0).powf(SHAPE)
            };
            let tau = if target > self.level[i] {
                ATTACK_TAU
            } else {
                DECAY_TAU
            };
            let alpha = 1.0 - (-dt / tau).exp();
            self.level[i] += (target - self.level[i]) * alpha;
        }
    }
}

// ---- shared state and the handle ---------------------------------------

/// What the worker publishes and the UI reads, all with relaxed atomics.
pub struct EqShared {
    levels: [AtomicU32; BANDS],
    /// A device is open and the levels are real. False means the bars use
    /// the sine fallback.
    pub live: AtomicBool,
    /// The output is muted, or its master volume is (nearly) 0.
    pub muted: AtomicBool,
}

impl EqShared {
    fn new() -> Self {
        Self {
            levels: std::array::from_fn(|_| AtomicU32::new(0)),
            live: AtomicBool::new(false),
            muted: AtomicBool::new(false),
        }
    }

    /// The five levels as last published.
    pub fn levels(&self) -> [f32; BANDS] {
        std::array::from_fn(|i| f32::from_bits(self.levels[i].load(Ordering::Relaxed)))
    }

    fn store(&self, levels: [f32; BANDS]) {
        for (slot, level) in self.levels.iter().zip(levels) {
            slot.store(level.to_bits(), Ordering::Relaxed);
        }
    }

    /// A session just opened: the levels are real from now on, and `muted`
    /// is the new device's own state, read at open.
    fn opened(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        self.live.store(true, Ordering::Relaxed);
    }

    /// No session: sine fallback, zero levels, and no mute carried over to
    /// the next device.
    fn reset(&self) {
        self.live.store(false, Ordering::Relaxed);
        self.muted.store(false, Ordering::Relaxed);
        self.store([0.0; BANDS]);
    }
}

/// The worker's handle. Dropping it ends the thread.
pub struct EqCapture {
    shared: Arc<EqShared>,
    control: Sender<bool>,
}

impl EqCapture {
    /// Starts the (stopped) worker; `None` when the thread could not start.
    pub fn spawn() -> Option<Self> {
        let shared = Arc::new(EqShared::new());
        let (control, rx) = mpsc::channel();
        let worker = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("eq-capture".into())
            .spawn(move || serve(&rx, &worker));
        match spawned {
            Ok(_) => Some(Self { shared, control }),
            Err(error) => {
                eprintln!("vale: no equalizer capture thread: {error}");
                None
            }
        }
    }

    /// Starts or stops the capture. Never blocks; false when the thread is gone.
    pub fn set_running(&self, on: bool) -> bool {
        self.control.send(on).is_ok()
    }

    pub fn shared(&self) -> &Arc<EqShared> {
        &self.shared
    }
}

// ---- the worker thread --------------------------------------------------

/// How often the loop looks for packets and control messages.
const POLL: Duration = Duration::from_millis(15);
/// A failed open is tried again at most this often while still running.
const RETRY: Duration = Duration::from_secs(2);
/// A capture error reopens after this short pause, so a broken device
/// cannot spin the thread.
const REOPEN_PAUSE: Duration = Duration::from_millis(250);
const MUTE_EVERY: Duration = Duration::from_millis(250);
const DEVICE_EVERY: Duration = Duration::from_secs(1);
/// Packets taken per tick at most (a 100 ms buffer holds about ten).
const MAX_PACKETS: usize = 64;

fn serve(rx: &Receiver<bool>, shared: &EqShared) {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let mut session: Option<Session> = None;
    let mut running = false;
    let mut next_open = Instant::now();
    loop {
        if !running || !com {
            // Stopped (or no COM): no client held, no polling, just wait.
            session = None;
            shared.reset();
            match rx.recv() {
                Ok(on) => running = last_message(rx, on),
                Err(_) => break,
            }
            next_open = Instant::now();
            continue;
        }
        match rx.recv_timeout(POLL) {
            Ok(on) => {
                running = last_message(rx, on);
                if !running {
                    continue;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        if session.is_none() {
            if now < next_open {
                continue;
            }
            match Session::open(now) {
                Some(s) => {
                    shared.opened(s.read_mute());
                    session = Some(s);
                }
                None => {
                    shared.reset();
                    next_open = now + RETRY;
                    continue;
                }
            }
        }
        let outcome = session.as_mut().map(|s| s.tick(now, shared));
        match outcome {
            Some(Tick::Ok) | None => {}
            Some(Tick::DeviceChanged) => {
                session = None;
                shared.reset();
                next_open = now;
            }
            Some(Tick::Failed) => {
                session = None;
                shared.reset();
                next_open = now + REOPEN_PAUSE;
            }
        }
    }
    drop(session);
    if com {
        unsafe { CoUninitialize() };
    }
}

/// The newest control message: `first` and whatever is queued behind it.
fn last_message(rx: &Receiver<bool>, first: bool) -> bool {
    rx.try_iter().last().unwrap_or(first)
}

enum Tick {
    Ok,
    /// The default output is another device now.
    DeviceChanged,
    /// A capture call failed (the device went away or was invalidated).
    Failed,
}

/// One open loopback stream on the default output device.
struct Session {
    enumerator: IMMDeviceEnumerator,
    id: String,
    client: IAudioClient,
    capture: IAudioCaptureClient,
    volume: Option<IAudioEndpointVolume>,
    fmt: SampleFormat,
    analyzer: Analyzer,
    mono: Vec<f32>,
    last_tick: Instant,
    last_mute: Instant,
    last_device: Instant,
}

impl Session {
    fn open(now: Instant) -> Option<Self> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .ok()?;
            let id = device_id(&device)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).ok()?;
            let wfx = client.GetMixFormat().ok()?;
            let parsed = read_format(wfx);
            // Shared mode, loopback on the render endpoint, a 100 ms buffer.
            let init = parsed.map(|_| {
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    1_000_000,
                    0,
                    wfx,
                    None,
                )
            });
            CoTaskMemFree(Some(wfx.cast_const().cast()));
            let (fmt, rate) = parsed?;
            init?.ok()?;
            let capture: IAudioCaptureClient = client.GetService().ok()?;
            let volume = device
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .ok();
            client.Start().ok()?;
            Some(Self {
                enumerator,
                id,
                client,
                capture,
                volume,
                fmt,
                analyzer: Analyzer::new(rate),
                mono: Vec::new(),
                last_tick: now,
                last_mute: now,
                last_device: now,
            })
        }
    }

    /// Takes the packets that are waiting, advances the analyzer by the time
    /// since the last tick and publishes the levels.
    fn tick(&mut self, now: Instant, shared: &EqShared) -> Tick {
        if now.duration_since(self.last_device) >= DEVICE_EVERY {
            self.last_device = now;
            // A default switch does not invalidate the old device, so the id
            // has to be compared. No default at all counts as a change.
            let current = unsafe {
                self.enumerator
                    .GetDefaultAudioEndpoint(eRender, eMultimedia)
                    .ok()
                    .and_then(|d| device_id(&d))
            };
            if current.as_deref() != Some(self.id.as_str()) {
                return Tick::DeviceChanged;
            }
        }
        if self.drain().is_err() {
            return Tick::Failed;
        }
        if now.duration_since(self.last_mute) >= MUTE_EVERY {
            self.last_mute = now;
            shared.muted.store(self.read_mute(), Ordering::Relaxed);
        }
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.25);
        self.last_tick = now;
        let muted = shared.muted.load(Ordering::Relaxed);
        shared.store(self.analyzer.advance(dt, muted));
        Tick::Ok
    }

    /// Reads every waiting packet into the analyzer; a silent packet is zeros.
    fn drain(&mut self) -> windows::core::Result<()> {
        for _ in 0..MAX_PACKETS {
            unsafe {
                if self.capture.GetNextPacketSize()? == 0 {
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let frames_n = frames as usize;
                self.mono.clear();
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    self.mono.resize(frames_n, 0.0);
                } else {
                    let bytes = std::slice::from_raw_parts(data, frames_n * self.fmt.block_align);
                    mix_to_mono(bytes, &self.fmt, &mut self.mono);
                }
                self.analyzer.push(&self.mono);
                self.capture.ReleaseBuffer(frames)?;
            }
        }
        Ok(())
    }

    /// Muted, or the master volume is as good as 0 (the loopback is taken
    /// before the endpoint volume, so it still plays in both cases).
    fn read_mute(&self) -> bool {
        let Some(volume) = &self.volume else {
            return false;
        };
        unsafe {
            volume.GetMute().is_ok_and(|m| m.as_bool())
                || volume
                    .GetMasterVolumeLevelScalar()
                    .is_ok_and(|v| v <= 0.001)
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

/// The endpoint id string of `device`.
fn device_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let raw = device.GetId().ok()?;
        let id = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0.cast_const().cast()));
        id
    }
}

/// The mix format as the analysis reads it, with its sample rate. The
/// structure is packed, so every field is read unaligned. Also used by the
/// recorder's capture worker (`recorder::capture`).
pub(crate) unsafe fn read_format(wfx: *const WAVEFORMATEX) -> Option<(SampleFormat, u32)> {
    unsafe {
        let tag = std::ptr::addr_of!((*wfx).wFormatTag).read_unaligned();
        let channels = std::ptr::addr_of!((*wfx).nChannels).read_unaligned();
        let rate = std::ptr::addr_of!((*wfx).nSamplesPerSec).read_unaligned();
        let align = std::ptr::addr_of!((*wfx).nBlockAlign).read_unaligned();
        let bits = std::ptr::addr_of!((*wfx).wBitsPerSample).read_unaligned();
        let size = std::ptr::addr_of!((*wfx).cbSize).read_unaligned();
        let sub = (tag == TAG_EXTENSIBLE && usize::from(size) >= 22).then(|| {
            let ext = wfx.cast::<WAVEFORMATEXTENSIBLE>();
            std::ptr::addr_of!((*ext).SubFormat).read_unaligned()
        });
        parse_format(tag, channels, bits, align, sub).map(|fmt| (fmt, rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: u32 = 48_000;

    fn sine(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (FS as f32 * secs) as usize;
        (0..n)
            .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / FS as f32).sin())
            .collect()
    }

    fn centre(band: usize) -> f32 {
        band_centre(band, FS as f32)
    }

    /// The five band centres at once, `amps` each, `secs` long, with an
    /// optional slow amplitude wobble (a stand-in for a song).
    fn song(amps: [f32; BANDS], secs: f32, wobble: bool) -> Vec<f32> {
        let n = (FS as f32 * secs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / FS as f32;
                let env = if wobble {
                    (1.0 + 0.5 * (std::f32::consts::TAU * 2.0 * t).sin()) / 1.5
                } else {
                    1.0
                };
                let mut v = 0.0;
                for b in 0..BANDS {
                    v += amps[b] * (std::f32::consts::TAU * centre(b) * t).sin();
                }
                v * env
            })
            .collect()
    }

    /// Plays `signal` in 10 ms chunks, one tick per chunk, and returns the
    /// levels after each tick.
    fn play(an: &mut Analyzer, signal: &[f32]) -> Vec<[f32; BANDS]> {
        signal
            .chunks((FS / 100) as usize)
            .map(|chunk| {
                an.push(chunk);
                an.advance(0.01, false)
            })
            .collect()
    }

    fn silence(an: &mut Analyzer, ticks: usize) -> [f32; BANDS] {
        let mut last = an.levels();
        for _ in 0..ticks {
            last = an.advance(0.01, false);
        }
        last
    }

    fn argmax(levels: [f32; BANDS]) -> usize {
        (0..BANDS)
            .max_by(|&a, &b| levels[a].total_cmp(&levels[b]))
            .unwrap()
    }

    #[test]
    fn a_sine_at_each_band_centre_lights_that_band_most() {
        let amps = [0.1; BANDS];
        for band in 0..BANDS {
            // Each band keeps its own auto-gain, so the analyzer first hears
            // the whole spectrum (which music always has), then one tone.
            let mut an = Analyzer::new(FS);
            play(&mut an, &song(amps, 1.0, false));
            let levels = *play(&mut an, &sine(centre(band), 0.1, 0.5)).last().unwrap();
            assert_eq!(argmax(levels), band, "band {band}: {levels:?}");
            assert!(levels[band] > 0.7, "band {band}: {levels:?}");
            for (other, level) in levels.iter().enumerate() {
                if other != band {
                    assert!(*level < levels[band] - 0.2, "band {band}: {levels:?}");
                }
            }
        }
    }

    #[test]
    fn the_filters_pass_their_own_centre_at_full_gain() {
        for band in 0..BANDS {
            let mut an = Analyzer::new(FS);
            an.push(&sine(centre(band), 0.5, 0.5));
            let rms = an.pending.last().expect("a hop closed")[band];
            let expect = 0.5 / 2f32.sqrt();
            assert!(
                (rms - expect).abs() < 0.02,
                "band {band}: {rms} vs {expect}"
            );
        }
    }

    #[test]
    fn the_centre_stays_below_the_nyquist_limit() {
        for rate in [8_000.0, 11_025.0, 22_050.0, 44_100.0, 96_000.0] {
            for band in 0..BANDS {
                assert!(band_centre(band, rate) <= 0.45 * rate + 1e-3);
            }
        }
        // A low rate still builds finite filters and finite levels.
        let mut an = Analyzer::new(8_000);
        an.push(&vec![0.3; 800]);
        assert!(an.advance(0.1, false).iter().all(|l| l.is_finite()));
    }

    #[test]
    fn the_attack_is_quick() {
        let mut an = Analyzer::new(FS);
        an.push(&sine(centre(2), 0.3, 0.035));
        let levels = an.advance(0.035, false);
        assert!(levels[2] >= 0.6, "{levels:?}");
    }

    #[test]
    fn the_decay_is_slower_than_the_attack() {
        let mut an = Analyzer::new(FS);
        play(&mut an, &sine(centre(2), 0.3, 0.5));
        assert!(an.levels()[2] > 0.95);
        // Silence delivers no samples at all: the bars fall on their own.
        let after_100ms = silence(&mut an, 10)[2];
        assert!(after_100ms > 0.3, "{after_100ms}");
        let after_1s = silence(&mut an, 90)[2];
        assert!(after_1s < 0.05, "{after_1s}");
        // Zeros pushed explicitly fall the same way.
        let mut an = Analyzer::new(FS);
        play(&mut an, &sine(centre(2), 0.3, 0.5));
        let levels = play(&mut an, &vec![0.0; FS as usize / 10]);
        assert!(levels.last().unwrap()[2] > 0.3);
        let levels = play(&mut an, &vec![0.0; FS as usize]);
        assert!(levels.last().unwrap()[2] < 0.05);
    }

    #[test]
    fn auto_gain_makes_a_quiet_and_a_loud_song_comparable() {
        let amps = [0.5, 0.4, 0.3, 0.3, 0.25];
        let mean_tail = |gain: f32| {
            let mut an = Analyzer::new(FS);
            let scaled = amps.map(|a| a * gain);
            let levels = play(&mut an, &song(scaled, 6.0, true));
            let tail = &levels[levels.len() - 200..];
            std::array::from_fn::<f32, BANDS, _>(|b| {
                tail.iter().map(|l| l[b]).sum::<f32>() / tail.len() as f32
            })
        };
        let quiet = mean_tail(0.1); // -20 dB
        let loud = mean_tail(0.5); // -6 dB
        for b in 0..BANDS {
            assert!(
                (quiet[b] - loud[b]).abs() < 0.1,
                "band {b}: {quiet:?} vs {loud:?}"
            );
            assert!(quiet[b] > 0.2, "band {b} should move: {quiet:?}");
        }
    }

    #[test]
    fn noise_at_minus_80_dbfs_stays_at_rest() {
        // Uniform noise with an RMS of 1e-4 (-80 dBFS), from a fixed LCG.
        let mut state = 0x2545_f491_u32;
        let noise: Vec<f32> = (0..FS as usize * 3)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let u = (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                u * 3f32.sqrt() * 1e-4
            })
            .collect();
        let mut an = Analyzer::new(FS);
        for levels in play(&mut an, &noise) {
            assert!(levels.iter().all(|&l| l == 0.0), "{levels:?}");
        }
    }

    #[test]
    fn muted_or_silent_input_goes_to_zero() {
        let mut an = Analyzer::new(FS);
        play(&mut an, &song([0.2; BANDS], 1.0, false));
        assert!(an.levels().iter().all(|&l| l > 0.5));
        // Loopback still delivers the stream while muted; the bars fall.
        let mut last = an.levels();
        for chunk in song([0.2; BANDS], 1.5, false).chunks((FS / 100) as usize) {
            an.push(chunk);
            last = an.advance(0.01, true);
        }
        assert!(last.iter().all(|&l| l < 0.05), "{last:?}");
        // Unmuting brings them back at once (the auto-gain kept its peaks).
        let back = *play(&mut an, &song([0.2; BANDS], 0.2, false))
            .last()
            .unwrap();
        assert!(back.iter().all(|&l| l > 0.5), "{back:?}");
        // Zero input never lights anything.
        let mut an = Analyzer::new(FS);
        let levels = play(&mut an, &vec![0.0; FS as usize]);
        assert!(levels.iter().all(|l| l.iter().all(|&v| v == 0.0)));
    }

    #[test]
    fn a_new_session_never_inherits_the_old_mute() {
        let shared = EqShared::new();
        // A muted device was open, then the session closed.
        shared.opened(true);
        shared.store([0.7; BANDS]);
        shared.reset();
        assert!(!shared.live.load(Ordering::Relaxed));
        assert!(!shared.muted.load(Ordering::Relaxed));
        assert_eq!(shared.levels(), [0.0; BANDS]);
        // The next device's own mute state is there from the first tick.
        shared.opened(true);
        assert!(shared.live.load(Ordering::Relaxed));
        assert!(shared.muted.load(Ordering::Relaxed));
        shared.reset();
        shared.opened(false);
        assert!(shared.live.load(Ordering::Relaxed));
        assert!(!shared.muted.load(Ordering::Relaxed));
    }

    #[test]
    fn bad_samples_and_steps_are_harmless() {
        let mut an = Analyzer::new(FS);
        an.push(&[f32::NAN, f32::INFINITY, -f32::INFINITY, 0.0]);
        an.push(&sine(centre(1), 0.3, 0.1));
        for dt in [f32::NAN, -1.0, 0.0, f32::INFINITY, 1e9] {
            let levels = an.advance(dt, false);
            assert!(
                levels.iter().all(|l| (0.0..=1.0).contains(l)),
                "{dt}: {levels:?}"
            );
        }
    }

    #[test]
    fn pending_hops_are_capped() {
        let mut an = Analyzer::new(FS);
        an.push(&sine(centre(1), 0.3, 5.0));
        assert_eq!(an.pending.len(), MAX_PENDING);
    }

    #[test]
    fn parse_format_reads_the_formats_the_mixer_gives() {
        let f = |tag, ch, bits, align, sub| parse_format(tag, ch, bits, align, sub);
        let fmt = |sample, channels, block_align| {
            Some(SampleFormat {
                sample,
                channels,
                block_align,
            })
        };
        assert_eq!(f(3, 2, 32, 8, None), fmt(Sample::F32, 2, 8));
        assert_eq!(
            f(0xFFFE, 2, 32, 8, Some(SUBTYPE_FLOAT)),
            fmt(Sample::F32, 2, 8)
        );
        assert_eq!(f(1, 2, 16, 4, None), fmt(Sample::I16, 2, 4));
        assert_eq!(
            f(0xFFFE, 6, 16, 12, Some(SUBTYPE_PCM)),
            fmt(Sample::I16, 6, 12)
        );
        assert_eq!(f(1, 2, 24, 6, None), fmt(Sample::I24, 2, 6));
        assert_eq!(
            f(0xFFFE, 2, 24, 6, Some(SUBTYPE_PCM)),
            fmt(Sample::I24, 2, 6)
        );
        assert_eq!(
            f(0xFFFE, 2, 32, 8, Some(SUBTYPE_PCM)),
            fmt(Sample::I32, 2, 8)
        );
        assert_eq!(f(3, 1, 32, 4, None), fmt(Sample::F32, 1, 4));
    }

    #[test]
    fn parse_format_refuses_everything_else() {
        let other = GUID::from_u128(0x0000_0002_0000_0010_8000_00aa_0038_9b71);
        // ADPCM, A-law, 8 bit, 64 bit float, float in 16 bits.
        assert_eq!(parse_format(2, 2, 4, 8, None), None);
        assert_eq!(parse_format(6, 2, 8, 2, None), None);
        assert_eq!(parse_format(1, 2, 8, 2, None), None);
        assert_eq!(parse_format(3, 2, 64, 16, None), None);
        assert_eq!(parse_format(3, 2, 16, 4, None), None);
        // Extensible with no or an unknown subformat.
        assert_eq!(parse_format(0xFFFE, 2, 32, 8, None), None);
        assert_eq!(parse_format(0xFFFE, 2, 32, 8, Some(other)), None);
        // No channels, or a frame too small for its samples.
        assert_eq!(parse_format(3, 0, 32, 0, None), None);
        assert_eq!(parse_format(1, 2, 16, 3, None), None);
    }

    fn mono_of(bytes: &[u8], fmt: SampleFormat) -> Vec<f32> {
        let mut out = Vec::new();
        mix_to_mono(bytes, &fmt, &mut out);
        out
    }

    #[test]
    fn mix_to_mono_averages_and_scales_each_format() {
        let fmt = |sample, channels| SampleFormat {
            sample,
            channels,
            block_align: sample.bytes() * channels,
        };
        // Float stereo: (1.0 + 0.0) / 2 and (-0.5 + -0.5) / 2.
        let mut b = Vec::new();
        for v in [1.0f32, 0.0, -0.5, -0.5] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(mono_of(&b, fmt(Sample::F32, 2)), vec![0.5, -0.5]);
        // 16 bit: full scale and half negative.
        let mut b = Vec::new();
        for v in [i16::MIN, i16::MIN, 16_384, 16_384] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(mono_of(&b, fmt(Sample::I16, 2)), vec![-1.0, 0.5]);
        // 24 bit packed: -8388608 (00 00 80) and +4194304 (00 00 40).
        let b = [0x00, 0x00, 0x80, 0x00, 0x00, 0x40];
        assert_eq!(mono_of(&b, fmt(Sample::I24, 1)), vec![-1.0, 0.5]);
        // 32 bit integer container.
        let mut b = Vec::new();
        for v in [i32::MIN, 1 << 30] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(mono_of(&b, fmt(Sample::I32, 1)), vec![-1.0, 0.5]);
        // A trailing partial frame is ignored and the output is appended to.
        let mut out = vec![9.0];
        mix_to_mono(&[0x00, 0x40, 0x00], &fmt(Sample::I16, 1), &mut out);
        assert_eq!(out, vec![9.0, 0.5]);
        // Not-a-number floats read as silence.
        let b = f32::NAN.to_le_bytes();
        assert_eq!(mono_of(&b, fmt(Sample::F32, 1)), vec![0.0]);
        assert!(mono_of(&[], fmt(Sample::F32, 2)).is_empty());
    }
}
