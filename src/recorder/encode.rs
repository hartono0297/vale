//! The `.m4a` writer: AAC-LC in MP4 through the Media Foundation sink writer,
//! the "Audio file" contract of the recording spec (48 kHz, 96 kbps stereo with
//! channel 0 = mic and channel 1 = system, or 64 kbps mono).
//!
//! - **Threads and COM.** Any thread may own a writer, but the caller must have
//!   initialised COM on it first (MTA: `CoInitializeEx(None,
//!   COINIT_MULTITHREADED)`), and create, write and finish on that same thread.
//!   Each writer runs its own `MFStartup(MF_VERSION, MFSTARTUP_LITE)` and the
//!   matching `MFShutdown` when it is finished or dropped.
//! - **No hard import of Media Foundation.** `mfplat.dll` and `mfreadwrite.dll`
//!   are loaded at run time: Windows N editions without the Media Feature Pack
//!   lack them, and a static import would stop Vale from starting at all there.
//!   Missing DLLs or a failing `MFStartup` give [`EncodeError::NoMediaFoundation`].
//! - **Container.** Fragmented MP4 first (a crash leaves a playable file), plain
//!   MP4 when the sink writer refuses it; [`M4aWriter::container`] tells which.
//!   The container is set by attribute, so the path's extension does not matter
//!   (the caller writes `<name>.m4a.part` and renames it after `finish`).
//! - **Input.** 32-bit float PCM; if the encoder chain refuses float, the writer
//!   converts to 16-bit PCM itself. Samples hold up to [`FRAMES_PER_SAMPLE`]
//!   frames (one AAC frame) and are stamped in 100 ns units from the running
//!   frame count, so timestamps never drift.
//! - **Nothing written.** `finish` on a writer that got no frames returns
//!   [`EncodeError::Empty`] and removes the file it created. A writer dropped
//!   without `finish` is not finalized; its file is left for the caller.

use std::ffi::c_void;
use std::fmt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::time::Duration;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSinkWriter, MF_MT_AAC_PAYLOAD_TYPE,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE,
    MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_TRANSCODE_CONTAINERTYPE, MF_VERSION, MFAudioFormat_AAC,
    MFAudioFormat_Float, MFAudioFormat_PCM, MFMediaType_Audio, MFSTARTUP_LITE,
    MFTranscodeContainerType_FMPEG4, MFTranscodeContainerType_MPEG4,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{GUID, HRESULT, HSTRING, Interface, PCWSTR, s, w};

/// The only sample rate the spec allows.
pub const RATE: u32 = 48_000;
/// Frames per sample handed to the sink writer: one AAC frame.
pub const FRAMES_PER_SAMPLE: usize = 1024;
/// 100 ns units per second (Media Foundation time).
const HNS_PER_SEC: u64 = 10_000_000;

/// Why a writer could not be made or could not write.
#[derive(Debug)]
pub enum EncodeError {
    /// Media Foundation is missing (Windows N without the Media Feature Pack)
    /// or `MFStartup` failed.
    NoMediaFoundation,
    /// A channel count other than 1 or 2, a rate other than 48000, or a
    /// buffer whose length is not a multiple of the channel count.
    BadFormat,
    /// `finish` was called before any frame was written; the file is removed.
    Empty,
    /// A Media Foundation call failed (sink writer, encoder or file).
    Io(windows::core::Error),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMediaFoundation => f.write_str("Media Foundation is not available"),
            Self::BadFormat => f.write_str("unsupported audio format"),
            Self::Empty => f.write_str("no audio was written"),
            Self::Io(e) => write!(f, "audio encoder failed: {e}"),
        }
    }
}

impl std::error::Error for EncodeError {}

impl From<windows::core::Error> for EncodeError {
    fn from(e: windows::core::Error) -> Self {
        Self::Io(e)
    }
}

/// The MP4 flavour the sink writer accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    /// Fragmented MP4 (`MFTranscodeContainerType_FMPEG4`).
    Fragmented,
    /// Plain MP4 (`MFTranscodeContainerType_MPEG4`), the fallback.
    Mpeg4,
}

impl Container {
    fn guid(self) -> GUID {
        match self {
            Self::Fragmented => MFTranscodeContainerType_FMPEG4,
            Self::Mpeg4 => MFTranscodeContainerType_MPEG4,
        }
    }
}

/// The PCM layout handed to the sink writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Input {
    Float,
    Pcm16,
}

impl Input {
    fn bytes(self) -> u32 {
        match self {
            Self::Float => 4,
            Self::Pcm16 => 2,
        }
    }
}

type StartupFn = unsafe extern "system" fn(u32, u32) -> HRESULT;
type ShutdownFn = unsafe extern "system" fn() -> HRESULT;
type CreateAttributesFn = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type CreateObjectFn = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateMemoryBufferFn = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
type CreateSinkWriterFn =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

/// The Media Foundation entry points, resolved at run time.
struct Mf {
    startup: StartupFn,
    shutdown: ShutdownFn,
    create_attributes: CreateAttributesFn,
    create_media_type: CreateObjectFn,
    create_sample: CreateObjectFn,
    create_memory_buffer: CreateMemoryBufferFn,
    create_sink_writer: CreateSinkWriterFn,
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
            create_attributes: std::mem::transmute(GetProcAddress(plat, s!("MFCreateAttributes"))?),
            create_media_type: std::mem::transmute(GetProcAddress(plat, s!("MFCreateMediaType"))?),
            create_sample: std::mem::transmute(GetProcAddress(plat, s!("MFCreateSample"))?),
            create_memory_buffer: std::mem::transmute(GetProcAddress(
                plat,
                s!("MFCreateMemoryBuffer"),
            )?),
            create_sink_writer: std::mem::transmute(GetProcAddress(
                rw,
                s!("MFCreateSinkWriterFromURL"),
            )?),
        })
    })
    .as_ref()
}

/// Calls a creator that returns an interface through an out pointer.
fn created<T: Interface>(
    call: impl FnOnce(*mut *mut c_void) -> HRESULT,
) -> windows::core::Result<T> {
    let mut raw = null_mut();
    call(&mut raw).ok()?;
    if raw.is_null() {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_POINTER,
        ));
    }
    // SAFETY: on success the creator stored an owned reference of type T.
    Ok(unsafe { T::from_raw(raw) })
}

/// One `MFStartup`, undone by `MFShutdown` on drop.
struct MfSession(&'static Mf);

impl MfSession {
    fn start() -> Result<Self, EncodeError> {
        let mf = mf().ok_or(EncodeError::NoMediaFoundation)?;
        unsafe { (mf.startup)(MF_VERSION, MFSTARTUP_LITE) }
            .ok()
            .map_err(|_| EncodeError::NoMediaFoundation)?;
        Ok(Self(mf))
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.0.shutdown)();
        }
    }
}

/// Writes interleaved f32 audio to an AAC `.m4a`.
pub struct M4aWriter {
    writer: IMFSinkWriter,
    stream: u32,
    channels: u16,
    rate: u32,
    input: Input,
    container: Container,
    /// Frames written so far; the next sample's time.
    frames: u64,
    path: PathBuf,
    /// Last field: dropped after `writer`, so `MFShutdown` runs once every
    /// Media Foundation object this writer held is released.
    mf: MfSession,
}

impl M4aWriter {
    /// Creates (or overwrites) `path` for `channels` (1 or 2) at `rate` (48000).
    /// COM must already be initialised on this thread (MTA).
    pub fn create(path: &Path, channels: u16, rate: u32) -> Result<Self, EncodeError> {
        Self::create_with(path, channels, rate, Input::Float)
    }

    /// `create` with the preferred input layout; tests force 16-bit PCM to
    /// cover the fallback path.
    fn create_with(
        path: &Path,
        channels: u16,
        rate: u32,
        prefer: Input,
    ) -> Result<Self, EncodeError> {
        if !(channels == 1 || channels == 2) || rate != RATE {
            return Err(EncodeError::BadFormat);
        }
        let mf = MfSession::start()?;
        let url = HSTRING::from(path);
        let mut last = None;
        for container in [Container::Fragmented, Container::Mpeg4] {
            match open(mf.0, &url, container, channels, rate, prefer) {
                Ok((writer, stream, input)) => {
                    return Ok(Self {
                        writer,
                        stream,
                        channels,
                        rate,
                        input,
                        container,
                        frames: 0,
                        path: path.to_path_buf(),
                        mf,
                    });
                }
                Err(e) => last = Some(e),
            }
        }
        Err(EncodeError::Io(last.expect("two attempts were made")))
    }

    /// The container the sink writer accepted.
    pub fn container(&self) -> Container {
        self.container
    }

    /// Appends interleaved samples (`channels` values per frame, -1..=1;
    /// louder values are clipped by the encoder). An empty slice does nothing.
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), EncodeError> {
        let channels = usize::from(self.channels);
        if interleaved.len() % channels != 0 {
            return Err(EncodeError::BadFormat);
        }
        for chunk in interleaved.chunks(FRAMES_PER_SAMPLE * channels) {
            self.write_chunk(chunk)?;
        }
        Ok(())
    }

    fn write_chunk(&mut self, chunk: &[f32]) -> Result<(), EncodeError> {
        let frames = (chunk.len() / usize::from(self.channels)) as u64;
        let len = chunk.len() as u32 * self.input.bytes();
        let mf = self.mf.0;
        unsafe {
            let buffer: IMFMediaBuffer = created(|p| (mf.create_memory_buffer)(len, p))?;
            let mut data = null_mut();
            buffer.Lock(&mut data, None, None)?;
            match self.input {
                Input::Float => {
                    std::ptr::copy_nonoverlapping(chunk.as_ptr().cast::<u8>(), data, len as usize)
                }
                Input::Pcm16 => {
                    let out = std::slice::from_raw_parts_mut(data.cast::<i16>(), chunk.len());
                    for (o, s) in out.iter_mut().zip(chunk) {
                        *o = to_pcm16(*s);
                    }
                }
            }
            buffer.Unlock()?;
            buffer.SetCurrentLength(len)?;
            let sample: IMFSample = created(|p| (mf.create_sample)(p))?;
            sample.AddBuffer(&buffer)?;
            let start = hns(self.frames, self.rate);
            let end = hns(self.frames + frames, self.rate);
            sample.SetSampleTime(start)?;
            sample.SetSampleDuration(end - start)?;
            self.writer.WriteSample(self.stream, &sample)?;
        }
        self.frames += frames;
        Ok(())
    }

    /// Finalizes the file and returns the duration of the audio written.
    /// With no frames written it removes the file and returns `Empty`.
    pub fn finish(self) -> Result<Duration, EncodeError> {
        if self.frames == 0 {
            let path = self.path.clone();
            drop(self);
            let _ = std::fs::remove_file(&path);
            return Err(EncodeError::Empty);
        }
        unsafe { self.writer.Finalize() }?;
        Ok(duration(self.frames, self.rate))
    }
}

/// Makes the sink writer for one container: the AAC stream and its input type,
/// then `BeginWriting`. Returns the writer, its stream index and the input used.
fn open(
    mf: &Mf,
    url: &HSTRING,
    container: Container,
    channels: u16,
    rate: u32,
    prefer: Input,
) -> windows::core::Result<(IMFSinkWriter, u32, Input)> {
    unsafe {
        let attrs: IMFAttributes = created(|p| (mf.create_attributes)(p, 1))?;
        attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &container.guid())?;
        let writer: IMFSinkWriter = created(|p| {
            (mf.create_sink_writer)(PCWSTR(url.as_ptr()), null_mut(), attrs.as_raw(), p)
        })?;

        let aac: IMFMediaType = created(|p| (mf.create_media_type)(p))?;
        aac.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        aac.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
        aac.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        aac.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
        aac.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))?;
        aac.SetUINT32(
            &MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
            aac_bytes_per_second(channels),
        )?;
        aac.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
        let stream = writer.AddStream(&aac)?;

        let set_input = |input: Input| -> windows::core::Result<()> {
            let ty: IMFMediaType = created(|p| (mf.create_media_type)(p))?;
            let block = u32::from(channels) * input.bytes();
            ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            ty.SetGUID(
                &MF_MT_SUBTYPE,
                match input {
                    Input::Float => &MFAudioFormat_Float,
                    Input::Pcm16 => &MFAudioFormat_PCM,
                },
            )?;
            ty.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, input.bytes() * 8)?;
            ty.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
            ty.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))?;
            ty.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block)?;
            ty.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * block)?;
            ty.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            writer.SetInputMediaType(stream, &ty, None::<&IMFAttributes>)
        };
        let input = match prefer {
            Input::Float => match set_input(Input::Float) {
                Ok(()) => Input::Float,
                Err(_) => {
                    set_input(Input::Pcm16)?;
                    Input::Pcm16
                }
            },
            Input::Pcm16 => {
                set_input(Input::Pcm16)?;
                Input::Pcm16
            }
        };
        writer.BeginWriting()?;
        Ok((writer, stream, input))
    }
}

/// AAC bitrate in bytes per second: 96 kbps stereo, 64 kbps mono.
fn aac_bytes_per_second(channels: u16) -> u32 {
    if channels == 2 { 12_000 } else { 8_000 }
}

/// `frames` at `rate` in 100 ns units.
fn hns(frames: u64, rate: u32) -> i64 {
    (frames * HNS_PER_SEC / u64::from(rate)) as i64
}

fn duration(frames: u64, rate: u32) -> Duration {
    Duration::from_nanos(frames * 1_000_000_000 / u64::from(rate))
}

/// f32 -1..=1 to 16-bit PCM, clipped; NaN gives 0.
fn to_pcm16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::f32::consts::TAU;
    use windows::Win32::Media::MediaFoundation::{
        MF_PD_DURATION, MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
        MF_SOURCE_READERF_ENDOFSTREAM, MFCreateMediaType, MFCreateSourceReaderFromURL, MFShutdown,
        MFStartup,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    /// A fresh folder under `target/test-tmp/` for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    fn com() {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    /// `seconds` of interleaved sines, one frequency per channel, amplitude 0.5.
    fn tones(freqs: &[f32], seconds: f32) -> Vec<f32> {
        let frames = (seconds * RATE as f32) as usize;
        let mut out = Vec::with_capacity(frames * freqs.len());
        for i in 0..frames {
            let t = i as f32 / RATE as f32;
            for f in freqs {
                out.push(0.5 * (TAU * f * t).sin());
            }
        }
        out
    }

    /// What the source reader decodes from a file.
    pub(crate) struct Decoded {
        pub(crate) channels: u32,
        pub(crate) rate: u32,
        pub(crate) samples: Vec<f32>,
        /// `MF_PD_DURATION`, when the container reports one.
        pub(crate) reported: Option<Duration>,
    }

    impl Decoded {
        pub(crate) fn frames(&self) -> usize {
            self.samples.len() / self.channels as usize
        }

        pub(crate) fn duration(&self) -> Duration {
            duration(self.frames() as u64, self.rate)
        }

        /// The frequency of one channel by zero crossings, skipping 100 ms at
        /// either end (encoder priming and the final frame).
        pub(crate) fn frequency(&self, channel: usize) -> f32 {
            let ch = self.channels as usize;
            let skip = self.rate as usize / 10;
            let mono: Vec<f32> = self
                .samples
                .iter()
                .skip(channel)
                .step_by(ch)
                .copied()
                .collect();
            let body = &mono[skip..mono.len() - skip];
            let crossings = body
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count() as f32;
            crossings / 2.0 / (body.len() as f32 / self.rate as f32)
        }
    }

    pub(crate) fn decode(path: &Path) -> Decoded {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_LITE).expect("MFStartup");
            let decoded = {
                let reader =
                    MFCreateSourceReaderFromURL(&HSTRING::from(path), None::<&IMFAttributes>)
                        .expect("source reader");
                let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
                let want = MFCreateMediaType().unwrap();
                want.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).unwrap();
                want.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float).unwrap();
                reader
                    .SetCurrentMediaType(stream, None, &want)
                    .expect("float output");
                let got = reader.GetCurrentMediaType(stream).unwrap();
                let channels = got.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).unwrap();
                let rate = got.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).unwrap();
                let reported = reader
                    .GetPresentationAttribute(
                        MF_SOURCE_READER_MEDIASOURCE.0 as u32,
                        &MF_PD_DURATION,
                    )
                    .ok()
                    .and_then(|v| u64::try_from(&v).ok())
                    .filter(|&d| d > 0)
                    .map(|d| Duration::from_nanos(d * 100));
                let mut samples = Vec::new();
                loop {
                    let mut flags = 0u32;
                    let mut sample: Option<IMFSample> = None;
                    reader
                        .ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
                        .expect("ReadSample");
                    if let Some(sample) = sample {
                        let buffer = sample.ConvertToContiguousBuffer().unwrap();
                        let mut data = null_mut();
                        let mut len = 0u32;
                        buffer.Lock(&mut data, None, Some(&mut len)).unwrap();
                        let floats =
                            std::slice::from_raw_parts(data.cast::<f32>(), len as usize / 4);
                        samples.extend_from_slice(floats);
                        buffer.Unlock().unwrap();
                    }
                    if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                        break;
                    }
                }
                Decoded {
                    channels,
                    rate,
                    samples,
                    reported,
                }
            };
            MFShutdown().unwrap();
            decoded
        }
    }

    fn assert_near(what: &str, got: Duration, want: Duration, tolerance: Duration) {
        let diff = got.abs_diff(want);
        assert!(
            diff <= tolerance,
            "{what}: {got:?} is not within {tolerance:?} of {want:?}"
        );
    }

    fn assert_freq(got: f32, want: f32) {
        assert!(
            (got - want).abs() <= want * 0.03,
            "dominant frequency {got} Hz, wanted {want} Hz"
        );
    }

    fn write_file(
        path: &Path,
        freqs: &[f32],
        seconds: f32,
        prefer: Input,
    ) -> (Duration, Container) {
        let mut w = M4aWriter::create_with(path, freqs.len() as u16, RATE, prefer).expect("create");
        let container = w.container();
        let audio = tones(freqs, seconds);
        // Uneven pieces (7 ms worth) so chunking and timestamps cross call borders.
        for piece in audio.chunks(336 * freqs.len()) {
            w.write(piece).expect("write");
        }
        let d = w.finish().expect("finish");
        (d, container)
    }

    #[test]
    fn stereo_tones_keep_duration_rate_and_channel_order() {
        com();
        let path = temp_dir("rec-encode-stereo").join("stereo.m4a");
        let (d, container) = write_file(&path, &[440.0, 880.0], 2.0, Input::Float);
        eprintln!("stereo container: {container:?}");
        assert_eq!(d, Duration::from_secs(2));
        let dec = decode(&path);
        assert_eq!((dec.channels, dec.rate), (2, 48_000));
        let tol = Duration::from_millis(50);
        assert_near("decoded", dec.duration(), Duration::from_secs(2), tol);
        if let Some(r) = dec.reported {
            assert_near("reported", r, Duration::from_secs(2), tol);
        }
        assert_freq(dec.frequency(0), 440.0);
        assert_freq(dec.frequency(1), 880.0);
    }

    #[test]
    fn mono_tone_through_a_part_file() {
        com();
        let dir = temp_dir("rec-encode-mono");
        let part = dir.join("mono.m4a.part");
        let (d, _) = write_file(&part, &[440.0], 1.0, Input::Float);
        assert_eq!(d, Duration::from_secs(1));
        let path = dir.join("mono.m4a");
        std::fs::rename(&part, &path).unwrap();
        let dec = decode(&path);
        assert_eq!((dec.channels, dec.rate), (1, 48_000));
        let tol = Duration::from_millis(50);
        assert_near("decoded", dec.duration(), Duration::from_secs(1), tol);
        if let Some(r) = dec.reported {
            assert_near("reported", r, Duration::from_secs(1), tol);
        }
        assert_freq(dec.frequency(0), 440.0);
    }

    #[test]
    fn pcm16_fallback_input_encodes_too() {
        com();
        let path = temp_dir("rec-encode-pcm").join("pcm.m4a");
        let (d, _) = write_file(&path, &[440.0, 880.0], 1.0, Input::Pcm16);
        assert_eq!(d, Duration::from_secs(1));
        let dec = decode(&path);
        assert_eq!((dec.channels, dec.rate), (2, 48_000));
        assert_near(
            "decoded",
            dec.duration(),
            Duration::from_secs(1),
            Duration::from_millis(50),
        );
        assert_freq(dec.frequency(0), 440.0);
        assert_freq(dec.frequency(1), 880.0);
    }

    #[test]
    fn float_input_is_taken_as_is() {
        com();
        let path = temp_dir("rec-encode-float").join("f.m4a");
        let w = M4aWriter::create(&path, 2, RATE).expect("create");
        eprintln!("input: {:?}, container: {:?}", w.input, w.container());
        assert_eq!(w.input, Input::Float);
        drop(w);
    }

    #[test]
    fn bad_channels_or_rate_are_refused() {
        com();
        let dir = temp_dir("rec-encode-bad");
        for (channels, rate) in [(0, RATE), (3, RATE), (2, 44_100), (1, 16_000)] {
            let r = M4aWriter::create(&dir.join("bad.m4a"), channels, rate);
            assert!(
                matches!(r, Err(EncodeError::BadFormat)),
                "{channels} ch {rate} Hz"
            );
        }
        assert!(!dir.join("bad.m4a").exists());
    }

    #[test]
    fn a_ragged_buffer_is_refused() {
        com();
        let path = temp_dir("rec-encode-ragged").join("r.m4a");
        let mut w = M4aWriter::create(&path, 2, RATE).expect("create");
        assert!(matches!(w.write(&[0.0; 3]), Err(EncodeError::BadFormat)));
        w.write(&tones(&[440.0, 880.0], 0.1)).unwrap();
        assert!(w.finish().is_ok());
    }

    #[test]
    fn finishing_with_nothing_written_is_empty_and_leaves_no_file() {
        com();
        let path = temp_dir("rec-encode-empty").join("e.m4a.part");
        let mut w = M4aWriter::create(&path, 1, RATE).expect("create");
        w.write(&[]).unwrap();
        assert!(matches!(w.finish(), Err(EncodeError::Empty)));
        assert!(!path.exists());
    }

    #[test]
    fn the_container_is_reported() {
        com();
        let path = temp_dir("rec-encode-container").join("c.m4a");
        let w = M4aWriter::create(&path, 1, RATE).expect("create");
        let c = w.container();
        eprintln!("container on this machine: {c:?}");
        assert!(matches!(c, Container::Fragmented | Container::Mpeg4));
    }

    #[test]
    fn helpers() {
        assert_eq!(hns(48_000, RATE), 10_000_000);
        assert_eq!(hns(1024, RATE), 213_333);
        assert_eq!(duration(96_000, RATE), Duration::from_secs(2));
        assert_eq!(aac_bytes_per_second(2), 12_000);
        assert_eq!(aac_bytes_per_second(1), 8_000);
        assert_eq!(to_pcm16(1.5), 32767);
        assert_eq!(to_pcm16(-1.0), -32767);
        assert_eq!(to_pcm16(f32::NAN), 0);
        assert_eq!(
            EncodeError::NoMediaFoundation.to_string(),
            "Media Foundation is not available"
        );
    }
}
