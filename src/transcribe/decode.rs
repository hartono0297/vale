//! Audio in: a recording (`.m4a`, AAC-LC in MP4, or a `.wav`) decoded by the
//! Media Foundation Source Reader to float samples, each channel resampled
//! on its own to 16 kHz mono f32, the only input the engine accepts.
//!
//! The decoder streams: `Decoder::next_block` hands out about a second at a
//! time, so an hour of stereo never sits in memory at 48 kHz. Track A's
//! files are 48 kHz; with both sources channel 0 is the mic and channel 1
//! the system sound, one source is mono.

use std::ffi::c_void;
use std::path::Path;
use std::sync::OnceLock;

use windows::Win32::Foundation::{E_POINTER, HMODULE};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaType, IMFSourceReader, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION,
    MFAudioFormat_Float, MFMediaType_Audio, MFSTARTUP_LITE,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{HRESULT, HSTRING, Interface, PCWSTR, s, w};

/// The engine's input rate.
pub const RATE: u32 = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// `mfplat.dll` / `mfreadwrite.dll` are missing (Windows N without the
    /// Media Feature Pack) or `MFStartup` failed.
    NoMediaFoundation,
    /// Media Foundation could not open the file (missing, not audio, no
    /// decoder).
    Open(String),
    Read(String),
    /// The audio stream has no channels or no rate.
    Format(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::NoMediaFoundation => write!(f, "no Media Foundation"),
            DecodeError::Open(why) => write!(f, "cannot open the audio: {why}"),
            DecodeError::Read(why) => write!(f, "cannot read the audio: {why}"),
            DecodeError::Format(why) => write!(f, "unsupported audio: {why}"),
        }
    }
}

type StartupFn = unsafe extern "system" fn(u32, u32) -> HRESULT;
type ShutdownFn = unsafe extern "system" fn() -> HRESULT;
type CreateObjectFn = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateSourceReaderFn =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;

/// The Media Foundation entry points, resolved at run time from System32:
/// a static import of `mfplat.dll` would stop `vale.exe` from starting at
/// all on Windows N without the Media Feature Pack (the recorder does the
/// same, `recorder::encode`). COM methods go through the crate as usual.
struct Mf {
    startup: StartupFn,
    shutdown: ShutdownFn,
    create_media_type: CreateObjectFn,
    create_source_reader: CreateSourceReaderFn,
}

/// Loads the two DLLs once; `None` when either or an export is missing.
fn mf() -> Option<&'static Mf> {
    static MF: OnceLock<Option<Mf>> = OnceLock::new();
    MF.get_or_init(|| unsafe {
        let plat: HMODULE =
            LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let rw: HMODULE =
            LoadLibraryExW(w!("mfreadwrite.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        // SAFETY: each target type is the documented signature of that export.
        Some(Mf {
            startup: std::mem::transmute(GetProcAddress(plat, s!("MFStartup"))?),
            shutdown: std::mem::transmute(GetProcAddress(plat, s!("MFShutdown"))?),
            create_media_type: std::mem::transmute(GetProcAddress(plat, s!("MFCreateMediaType"))?),
            create_source_reader: std::mem::transmute(GetProcAddress(
                rw,
                s!("MFCreateSourceReaderFromURL"),
            )?),
        })
    })
    .as_ref()
}

/// Calls a creator that returns an interface through an out pointer.
fn created<T: Interface>(
    call: impl FnOnce(*mut *mut c_void) -> HRESULT,
) -> windows::core::Result<T> {
    let mut raw = std::ptr::null_mut();
    call(&mut raw).ok()?;
    if raw.is_null() {
        return Err(windows::core::Error::from_hresult(E_POINTER));
    }
    // SAFETY: on success the creator stored an owned reference of type T.
    Ok(unsafe { T::from_raw(raw) })
}

/// Per-thread COM and Media Foundation, undone on drop in reverse order.
struct MfSession {
    mf: &'static Mf,
    com: bool,
}

impl MfSession {
    fn start() -> Result<Self, DecodeError> {
        let mf = mf().ok_or(DecodeError::NoMediaFoundation)?;
        // S_FALSE (already initialised) still needs its CoUninitialize;
        // RPC_E_CHANGED_MODE (an STA thread) works as is and must not get one.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        if unsafe { (mf.startup)(MF_VERSION, MFSTARTUP_LITE) }.is_err() {
            if com {
                unsafe { CoUninitialize() };
            }
            return Err(DecodeError::NoMediaFoundation);
        }
        Ok(MfSession { mf, com })
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.mf.shutdown)();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

/// A file being decoded. Not `Send`: open and read it on one thread.
pub struct Decoder {
    // Field order: the reader is released before Media Foundation shuts down.
    reader: IMFSourceReader,
    channels: usize,
    rate: u32,
    duration_ms: Option<u64>,
    resamplers: Vec<Resampler>,
    ended: bool,
    _mf: MfSession,
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Decoder, DecodeError> {
        let mf = MfSession::start()?;
        let open = |e: windows::core::Error| DecodeError::Open(e.message());
        let url = HSTRING::from(path.as_os_str());
        let reader: IMFSourceReader = created(|out| unsafe {
            (mf.mf.create_source_reader)(PCWSTR(url.as_ptr()), std::ptr::null_mut(), out)
        })
        .map_err(open)?;
        let audio = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        unsafe {
            reader
                .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                .map_err(open)?;
            reader.SetStreamSelection(audio, true).map_err(open)?;
            let wanted: IMFMediaType =
                created(|out| (mf.mf.create_media_type)(out)).map_err(open)?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(open)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float).map_err(open)?;
            reader
                .SetCurrentMediaType(audio, None, &wanted)
                .map_err(|e| DecodeError::Format(e.message()))?;
        }
        let duration_ms = unsafe {
            reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|value| u64::try_from(&value).ok())
                .map(|hns| hns / 10_000)
        };
        let mut decoder = Decoder {
            reader,
            channels: 0,
            rate: 0,
            duration_ms,
            resamplers: Vec::new(),
            ended: false,
            _mf: mf,
        };
        decoder.read_format()?;
        Ok(decoder)
    }

    /// Channels and rate of the decoded stream; resamplers to match.
    fn read_format(&mut self) -> Result<(), DecodeError> {
        let format = |e: windows::core::Error| DecodeError::Format(e.message());
        let current = unsafe {
            self.reader
                .GetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32)
        }
        .map_err(format)?;
        let channels = unsafe { current.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.map_err(format)?;
        let rate = unsafe { current.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.map_err(format)?;
        if channels == 0 || rate == 0 {
            return Err(DecodeError::Format(format!("{channels} channels at {rate} Hz")));
        }
        // A format change mid-file keeps whatever the old resamplers hold.
        let mut flushed = Vec::new();
        for resampler in &mut self.resamplers {
            resampler.flush(&mut flushed);
        }
        self.channels = channels as usize;
        self.rate = rate;
        self.resamplers = (0..self.channels).map(|_| Resampler::new(rate, RATE)).collect();
        Ok(())
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The source's own rate (48000 for a Vale recording).
    pub fn source_rate(&self) -> u32 {
        self.rate
    }

    /// From the container, when it says.
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// The next piece of audio, one 16 kHz vector per channel (equal
    /// lengths), or `None` at the end.
    pub fn next_block(&mut self) -> Result<Option<Vec<Vec<f32>>>, DecodeError> {
        let read = |e: windows::core::Error| DecodeError::Read(e.message());
        let mut out: Vec<Vec<f32>> = vec![Vec::new(); self.channels];
        // About a second of source audio per block.
        while !self.ended && out[0].len() < RATE as usize {
            let mut flags = 0u32;
            let mut sample = None;
            unsafe {
                self.reader
                    .ReadSample(
                        MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32,
                        0,
                        None,
                        Some(&raw mut flags),
                        None,
                        Some(&raw mut sample),
                    )
                    .map_err(read)?;
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                let before = self.channels;
                self.read_format()?;
                if self.channels != before {
                    return Err(DecodeError::Format("channel count changed".into()));
                }
            }
            if let Some(sample) = sample {
                let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(read)?;
                let mut data = std::ptr::null_mut();
                let mut len = 0u32;
                unsafe { buffer.Lock(&mut data, None, Some(&raw mut len)) }.map_err(read)?;
                let floats = unsafe {
                    std::slice::from_raw_parts(data as *const f32, len as usize / 4)
                };
                let planes = deinterleave(floats, self.channels);
                let _ = unsafe { buffer.Unlock() };
                for (resampler, (plane, out)) in
                    self.resamplers.iter_mut().zip(planes.iter().zip(out.iter_mut()))
                {
                    resampler.process(plane, out);
                }
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                self.ended = true;
                for (resampler, out) in self.resamplers.iter_mut().zip(out.iter_mut()) {
                    resampler.flush(out);
                }
            }
        }
        if out[0].is_empty() && self.ended {
            return Ok(None);
        }
        Ok(Some(out))
    }
}

/// Interleaved frames to one vector per channel (a short last frame is
/// dropped).
pub fn deinterleave(frames: &[f32], channels: usize) -> Vec<Vec<f32>> {
    let n = frames.len() / channels.max(1);
    let mut planes = vec![Vec::with_capacity(n); channels];
    for frame in frames.chunks_exact(channels.max(1)) {
        for (plane, &value) in planes.iter_mut().zip(frame) {
            plane.push(value);
        }
    }
    planes
}

/// Rational polyphase resampler with a Blackman-windowed sinc low-pass,
/// streaming: `process` any number of times, then `flush` once.
pub struct Resampler {
    /// Up by `up`, down by `down` (the rates over their gcd).
    up: u64,
    down: u64,
    /// Taps per phase (`2 * half`), one row per phase.
    half: usize,
    taps: Vec<Vec<f32>>,
    /// Input not yet fully used, starting at absolute input index `base`
    /// (which starts at `-half + 1`: the history is zero before the start).
    pending: Vec<f32>,
    base: i64,
    /// Next output index.
    next: u64,
    /// Input samples seen.
    seen: u64,
    passthrough: bool,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        let g = gcd(from as u64, to as u64).max(1);
        let (up, down) = (to as u64 / g, from as u64 / g);
        // Cutoff in cycles per input sample: 90 % of the lower Nyquist.
        let cutoff = 0.5 * (to.min(from) as f64 / from as f64) * 0.9;
        // Ten zero crossings of the sinc on each side.
        let half = ((10.0 / (2.0 * cutoff)).ceil() as usize).max(2);
        let taps = (0..up)
            .map(|phase| {
                let offset = phase as f64 / up as f64;
                let mut row: Vec<f64> = (0..2 * half)
                    .map(|j| {
                        // Distance from the output instant to input sample
                        // `i + k`, with k = j - half + 1.
                        let x = (j as f64 - half as f64 + 1.0) - offset;
                        let sinc = if x == 0.0 {
                            1.0
                        } else {
                            let a = std::f64::consts::PI * 2.0 * cutoff * x;
                            a.sin() / a
                        };
                        let w = x / half as f64;
                        let window = if w.abs() >= 1.0 {
                            0.0
                        } else {
                            let a = std::f64::consts::PI * w;
                            0.42 + 0.5 * a.cos() + 0.08 * (2.0 * a).cos()
                        };
                        sinc * window
                    })
                    .collect();
                let sum: f64 = row.iter().sum();
                row.iter_mut().for_each(|v| *v /= sum);
                row.into_iter().map(|v| v as f32).collect()
            })
            .collect();
        Resampler {
            up,
            down,
            half,
            taps,
            pending: vec![0.0; half - 1],
            base: -(half as i64) + 1,
            next: 0,
            seen: 0,
            passthrough: from == to,
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.seen += input.len() as u64;
        if self.passthrough {
            out.extend_from_slice(input);
            return;
        }
        self.pending.extend_from_slice(input);
        self.run(out, None);
    }

    /// Emits the tail: the input is taken as zero after its end, and the
    /// output stops at `seen * to / from` samples.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if self.passthrough {
            return;
        }
        let total = (self.seen * self.up).div_ceil(self.down);
        self.pending.extend(std::iter::repeat_n(0.0, self.half + 1));
        self.run(out, Some(total));
        self.pending.clear();
    }

    fn run(&mut self, out: &mut Vec<f32>, limit: Option<u64>) {
        let width = 2 * self.half;
        loop {
            if limit.is_some_and(|l| self.next >= l) {
                break;
            }
            let pos = self.next * self.down;
            let i = (pos / self.up) as i64;
            let phase = (pos % self.up) as usize;
            // Taps cover input samples i - half + 1 ..= i + half.
            let first = i - self.half as i64 + 1;
            let start = (first - self.base) as usize;
            if start + width > self.pending.len() {
                break;
            }
            let window = &self.pending[start..start + width];
            let acc: f32 = window
                .iter()
                .zip(&self.taps[phase])
                .map(|(x, h)| x * h)
                .sum();
            out.push(acc);
            self.next += 1;
        }
        // Drop input no later output needs.
        let pos = self.next * self.down;
        let first_needed = (pos / self.up) as i64 - self.half as i64 + 1;
        let drop = (first_needed - self.base).clamp(0, self.pending.len() as i64) as usize;
        if drop > 0 {
            self.pending.drain(..drop);
            self.base += drop as i64;
        }
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::transcribe::store::tests::temp_store;

    fn sine(freq: f64, rate: u32, seconds: f64, amp: f32) -> Vec<f32> {
        let n = (rate as f64 * seconds) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin() as f32)
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// The middle of a signal (skips the filter's edges).
    fn middle(x: &[f32]) -> &[f32] {
        &x[x.len() / 10..x.len() * 9 / 10]
    }

    fn resample_all(from: u32, to: u32, input: &[f32]) -> Vec<f32> {
        let mut r = Resampler::new(from, to);
        let mut out = Vec::new();
        r.process(input, &mut out);
        r.flush(&mut out);
        out
    }

    #[test]
    fn a_tone_in_band_keeps_its_level_and_length() {
        let input = sine(1000.0, 48_000, 1.0, 0.5);
        let out = resample_all(48_000, 16_000, &input);
        assert_eq!(out.len(), 16_000);
        let level = rms(middle(&out));
        assert!((level - 0.5 / 2f32.sqrt()).abs() < 0.01, "{level}");
        // About 2000 zero crossings a second for 1 kHz.
        let crossings = out.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((1990..=2010).contains(&crossings), "{crossings}");
    }

    #[test]
    fn a_tone_past_the_new_nyquist_is_filtered_out() {
        let input = sine(10_000.0, 48_000, 1.0, 0.5);
        let out = resample_all(48_000, 16_000, &input);
        let level = rms(middle(&out));
        assert!(level < 0.005, "aliased level {level}");
    }

    #[test]
    fn odd_ratios_work_too() {
        let input = sine(440.0, 44_100, 2.0, 0.3);
        let out = resample_all(44_100, 16_000, &input);
        assert_eq!(out.len(), 32_000);
        let level = rms(middle(&out));
        assert!((level - 0.3 / 2f32.sqrt()).abs() < 0.01, "{level}");
        let dc = resample_all(22_050, 16_000, &vec![0.25; 22_050]);
        assert!(middle(&dc).iter().all(|v| (v - 0.25).abs() < 1e-3));
        assert_eq!(resample_all(16_000, 16_000, &[0.1, 0.2]), vec![0.1, 0.2]);
    }

    #[test]
    fn streaming_in_pieces_equals_one_go() {
        let input = sine(700.0, 48_000, 0.5, 0.4);
        let whole = resample_all(48_000, 16_000, &input);
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        let mut at = 0;
        for size in [1, 7, 480, 1023, 5000].iter().cycle() {
            if at >= input.len() {
                break;
            }
            let end = (at + size).min(input.len());
            r.process(&input[at..end], &mut out);
            at = end;
        }
        r.flush(&mut out);
        assert_eq!(out.len(), whole.len());
        assert!(out.iter().zip(&whole).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn deinterleave_splits_frames() {
        let planes = deinterleave(&[1.0, -1.0, 2.0, -2.0, 3.0], 2);
        assert_eq!(planes, vec![vec![1.0, 2.0], vec![-1.0, -2.0]]);
    }

    /// A 16-bit PCM WAV file of `planes` (equal lengths) at `rate`.
    pub(crate) fn write_wav(path: &Path, rate: u32, planes: &[Vec<f32>]) {
        let channels = planes.len() as u16;
        let frames = planes[0].len();
        let data_len = (frames * planes.len() * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            for plane in planes {
                let v = (plane[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(path, out).unwrap();
    }

    fn decode_all(path: &Path) -> (usize, Option<u64>, Vec<Vec<f32>>) {
        let mut decoder = Decoder::open(path).unwrap();
        let channels = decoder.channels();
        let duration = decoder.duration_ms();
        let mut all = vec![Vec::new(); channels];
        while let Some(block) = decoder.next_block().unwrap() {
            assert!(block.iter().all(|c| c.len() == block[0].len()));
            for (all, block) in all.iter_mut().zip(block) {
                all.extend(block);
            }
        }
        (channels, duration, all)
    }

    #[test]
    fn a_stereo_wav_decodes_to_two_16k_channels() {
        let store = temp_store("decode-wav");
        let path = store.root().join("both.wav");
        let mic = sine(500.0, 48_000, 2.0, 0.5);
        let system = vec![0.0; mic.len()];
        write_wav(&path, 48_000, &[mic, system]);
        let (channels, duration, planes) = decode_all(&path);
        assert_eq!(channels, 2);
        assert_eq!(duration, Some(2000));
        assert!((planes[0].len() as i64 - 32_000).abs() <= 1, "{}", planes[0].len());
        assert!((rms(middle(&planes[0])) - 0.5 / 2f32.sqrt()).abs() < 0.01);
        assert!(rms(&planes[1]) < 1e-4, "the silent channel stays silent");
    }

    #[test]
    fn a_missing_file_is_an_open_error() {
        let store = temp_store("decode-missing");
        let result = Decoder::open(&store.root().join("nope.m4a"));
        assert!(matches!(result, Err(DecodeError::Open(_))));
    }

    /// Writes a 48 kHz `.m4a` (AAC-LC, the recorder's format) of `planes`
    /// through the Media Foundation Sink Writer.
    pub(crate) fn write_m4a(path: &Path, planes: &[Vec<f32>]) {
        use windows::Win32::Media::MediaFoundation::*;
        let _mf = MfSession::start().unwrap();
        let channels = planes.len() as u32;
        let frames = planes[0].len();
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 1).unwrap();
            let attributes = attributes.unwrap();
            attributes
                .SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)
                .unwrap();
            let writer =
                MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None, &attributes)
                    .unwrap();
            let output = MFCreateMediaType().unwrap();
            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).unwrap();
            output.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).unwrap();
            output.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000).unwrap();
            output.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels).unwrap();
            output.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).unwrap();
            output
                .SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 12_000)
                .unwrap();
            let stream = writer.AddStream(&output).unwrap();
            let input = MFCreateMediaType().unwrap();
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).unwrap();
            input.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).unwrap();
            input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000).unwrap();
            input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels).unwrap();
            input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).unwrap();
            input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, channels * 2).unwrap();
            input
                .SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 48_000 * channels * 2)
                .unwrap();
            writer.SetInputMediaType(stream, &input, None).unwrap();
            writer.BeginWriting().unwrap();
            let per_block = 4800; // 100 ms
            let mut at = 0;
            while at < frames {
                let n = per_block.min(frames - at);
                let bytes = (n * channels as usize * 2) as u32;
                let buffer = MFCreateMemoryBuffer(bytes).unwrap();
                let mut data = std::ptr::null_mut();
                buffer.Lock(&mut data, None, None).unwrap();
                let pcm = std::slice::from_raw_parts_mut(data as *mut i16, n * channels as usize);
                for i in 0..n {
                    for (c, plane) in planes.iter().enumerate() {
                        pcm[i * channels as usize + c] =
                            (plane[at + i].clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                }
                buffer.Unlock().unwrap();
                buffer.SetCurrentLength(bytes).unwrap();
                let sample = MFCreateSample().unwrap();
                sample.AddBuffer(&buffer).unwrap();
                sample.SetSampleTime(at as i64 * 10_000_000 / 48_000).unwrap();
                sample.SetSampleDuration(n as i64 * 10_000_000 / 48_000).unwrap();
                writer.WriteSample(stream, &sample).unwrap();
                at += n;
            }
            writer.Finalize().unwrap();
        }
    }

    #[test]
    fn a_recorder_style_m4a_keeps_mic_and_system_apart() {
        let store = temp_store("decode-m4a");
        let path = store.root().join("2026-11-07 1430 Meet.m4a");
        // Mic: a tone for the first second, then quiet. System: quiet,
        // then a tone in the second second.
        let n = 96_000;
        let mut mic = sine(440.0, 48_000, 2.0, 0.4);
        let mut system = sine(880.0, 48_000, 2.0, 0.4);
        mic[n / 2..].iter_mut().for_each(|v| *v = 0.0);
        system[..n / 2].iter_mut().for_each(|v| *v = 0.0);
        write_m4a(&path, &[mic, system]);

        let (channels, duration, planes) = decode_all(&path);
        assert_eq!(channels, 2);
        let duration = duration.unwrap();
        assert!((1990..=2100).contains(&duration), "{duration}");
        // AAC adds a little priming delay; compare the clear middles of each
        // second.
        let first = 4_000..12_000;
        let second = 20_000..28_000;
        assert!(rms(&planes[0][first.clone()]) > 0.2);
        assert!(rms(&planes[0][second.clone()]) < 0.02);
        assert!(rms(&planes[1][first]) < 0.02);
        assert!(rms(&planes[1][second]) > 0.2);
    }
}
