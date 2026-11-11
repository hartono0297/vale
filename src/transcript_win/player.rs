//! Plays a recording for its transcript window: a Media Foundation source
//! reader decodes the file to float PCM at its own rate, every frame is mixed
//! to the same sample in both ears (a mic-left / PC-right recording is not
//! one-sided) and written to a shared-mode WASAPI stream on the default
//! console endpoint, event driven.
//!
//! The `transcript-player` thread lives only while playing ([[bata-idle-power-budget]]):
//! `play_from` starts it, `pause` and `Drop` stop and join it. One plays at a
//! time across every window: starting one stops the others. Media Foundation
//! comes from `crate::mf`, never an import ([[media-foundation-dynamic-load]]).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient,
    IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaType, IMFSourceReader, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MFAudioFormat_Float, MFMediaType_Audio,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::{GUID, HSTRING, PCWSTR};

use crate::mf::{MfSession, created};

/// Posted to the window when playback ends (wparam 0) or fails (wparam 1).
pub const WM_APP_PLAYER: u32 = WM_APP + 86;

/// The render buffer: 100 ms.
const BUFFER_HNS: i64 = 1_000_000;
/// How long the thread waits for the device before it looks at the stop flag
/// again (the device signals every period, about 10 ms, while it plays).
const WAIT_MS: u32 = 200;
/// `WAVE_FORMAT_IEEE_FLOAT`.
const FORMAT_FLOAT: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerError {
    /// `mfplat.dll` / `mfreadwrite.dll` are missing or `MFStartup` failed.
    NoMediaFoundation,
    /// The file could not be opened or read.
    Open(String),
    /// The output device could not be opened or stopped working.
    Device(String),
}

/// What the window needs of a player (a fake stands in for it in the tests).
pub trait Playback {
    /// Plays from `ms`, stopping what this player (or any other) plays.
    /// `notify` gets `WM_APP_PLAYER`.
    fn play_from(&mut self, ms: u64, notify: HWND) -> Result<(), PlayerError>;
    /// Stops and joins the thread; the position stays.
    fn pause(&mut self);
    /// The thread runs: not paused, ended, failed or stopped by another one.
    fn playing(&self) -> bool;
    /// What has been heard so far.
    fn position_ms(&self) -> u64;
    /// The file's length, known once it was opened.
    fn duration_ms(&self) -> Option<u64>;
}

/// The stop flag of the thread that plays now, whichever player started it.
static ACTIVE: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

struct Run {
    stop: Arc<AtomicBool>,
    /// Set by the thread before it posts its last message and ends.
    done: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

pub struct Player {
    audio: PathBuf,
    position: Arc<AtomicU64>,
    duration: Option<u64>,
    run: Option<Run>,
}

impl Player {
    /// A paused player at 0; no thread.
    pub fn new(audio: PathBuf) -> Player {
        Player {
            audio,
            position: Arc::new(AtomicU64::new(0)),
            duration: None,
            run: None,
        }
    }
}

impl Playback for Player {
    fn play_from(&mut self, ms: u64, notify: HWND) -> Result<(), PlayerError> {
        self.pause();
        self.position.store(ms, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(false));
        if let Ok(mut active) = ACTIVE.lock()
            && let Some(other) = active.replace(stop.clone())
        {
            other.store(true, Ordering::Relaxed);
        }
        let done = Arc::new(AtomicBool::new(false));
        let job = Job {
            audio: self.audio.clone(),
            start_ms: ms,
            position: self.position.clone(),
            stop: stop.clone(),
            done: done.clone(),
            notify: notify.0 as isize,
        };
        let (ready, started) = sync_channel(1);
        let thread = thread::Builder::new()
            .name("transcript-player".into())
            .spawn(move || job.run(ready))
            .map_err(|e| PlayerError::Device(e.to_string()))?;
        match started.recv() {
            Ok(Ok(duration)) => {
                self.duration = duration.or(self.duration);
                self.run = Some(Run { stop, done, thread });
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(PlayerError::Device("the player thread ended".into()))
            }
        }
    }

    fn pause(&mut self) {
        if let Some(run) = self.run.take() {
            run.stop.store(true, Ordering::Relaxed);
            let _ = run.thread.join();
        }
    }

    fn playing(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| !run.done.load(Ordering::SeqCst))
    }

    fn position_ms(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    fn duration_ms(&self) -> Option<u64> {
        self.duration
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.pause();
    }
}

/// Every frame of `channels` interleaved samples becomes two equal samples,
/// the mean of its channels, appended to `out` (a short last frame is
/// dropped).
pub fn downmix(frames: &[f32], channels: usize, out: &mut Vec<f32>) {
    let channels = channels.max(1);
    out.reserve(frames.len() / channels * 2);
    for frame in frames.chunks_exact(channels) {
        let mean = frame.iter().sum::<f32>() / channels as f32;
        out.extend([mean, mean]);
    }
}

/// Where playback is after `frames` frames at `rate` from `start_ms`.
pub fn played_ms(start_ms: u64, frames: u64, rate: u32) -> u64 {
    if rate == 0 {
        return start_ms;
    }
    start_ms + frames * 1_000 / u64::from(rate)
}

// ------------------------------------------------------------------ thread

/// What the thread is given.
struct Job {
    audio: PathBuf,
    start_ms: u64,
    position: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    notify: isize,
}

type Ready = SyncSender<Result<Option<u64>, PlayerError>>;

impl Job {
    /// Opens the file and the device, says so through `ready` (with the
    /// length, or why not), plays to the end or the stop flag and tells the
    /// window how it ended.
    fn run(self, ready: Ready) {
        let ended = match MfSession::start() {
            None => {
                let _ = ready.send(Err(PlayerError::NoMediaFoundation));
                None
            }
            Some(mf) => {
                let opened = Source::open(&mf, &self.audio, self.start_ms)
                    .and_then(|source| Output::open(source.rate).map(|out| (source, out)));
                // The reader and the client are gone before `mf` shuts down.
                match opened {
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        None
                    }
                    Ok((source, out)) => {
                        let _ = ready.send(Ok(source.duration_ms));
                        Some(self.stream(source, out))
                    }
                }
            }
        };
        self.done.store(true, Ordering::SeqCst);
        let code = match ended {
            Some(Ok(true)) => 0,
            Some(Err(e)) => {
                eprintln!("vale: transcript player: {e:?}");
                1
            }
            _ => return,
        };
        if self.notify != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(self.notify as *mut _)),
                    WM_APP_PLAYER,
                    WPARAM(code),
                    LPARAM(0),
                );
            }
        }
    }

    /// Feeds the device until the file has been heard to its end (true) or
    /// the stop flag is set (false).
    fn stream(&self, mut source: Source, out: Output) -> Result<bool, PlayerError> {
        let dev = |e: windows::core::Error| PlayerError::Device(e.message());
        let mut pending: Vec<f32> = Vec::new();
        let mut written: u64 = 0;
        let mut started = false;
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let padding = unsafe { out.client.GetCurrentPadding() }.map_err(dev)?;
            let room = out.frames.saturating_sub(padding) as usize;
            while !source.ended && pending.len() / 2 < room {
                if self.stop.load(Ordering::Relaxed) {
                    return Ok(false);
                }
                source.read(&mut pending)?;
            }
            let n = room.min(pending.len() / 2);
            if n > 0 {
                unsafe {
                    let data = out.render.GetBuffer(n as u32).map_err(dev)?;
                    std::ptr::copy_nonoverlapping(pending.as_ptr(), data.cast::<f32>(), n * 2);
                    out.render.ReleaseBuffer(n as u32, 0).map_err(dev)?;
                }
                pending.drain(..n * 2);
                written += n as u64;
            }
            let queued = u64::from(padding) + n as u64;
            self.position.store(
                played_ms(self.start_ms, written.saturating_sub(queued), source.rate),
                Ordering::Relaxed,
            );
            if source.ended && pending.is_empty() && queued == 0 {
                return Ok(true);
            }
            if !started {
                unsafe { out.client.Start() }.map_err(dev)?;
                started = true;
            }
            unsafe {
                let _ = WaitForSingleObject(out.event.0, WAIT_MS);
            }
        }
    }
}

/// The file, decoded to float frames at its own rate.
struct Source {
    reader: IMFSourceReader,
    channels: usize,
    rate: u32,
    duration_ms: Option<u64>,
    /// Where playback starts (100 ns): a seek may land a little before it.
    from_hns: i64,
    ended: bool,
}

impl Source {
    fn open(mf: &MfSession, path: &std::path::Path, start_ms: u64) -> Result<Source, PlayerError> {
        let open = |e: windows::core::Error| PlayerError::Open(e.message());
        let url = HSTRING::from(path.as_os_str());
        let reader: IMFSourceReader = created(|out| unsafe {
            (mf.api().create_source_reader)(PCWSTR(url.as_ptr()), std::ptr::null_mut(), out)
        })
        .map_err(open)?;
        let audio = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        let from_hns = i64::try_from(start_ms.saturating_mul(10_000)).unwrap_or(i64::MAX);
        unsafe {
            reader
                .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                .map_err(open)?;
            reader.SetStreamSelection(audio, true).map_err(open)?;
            let wanted: IMFMediaType =
                created(|out| (mf.api().create_media_type)(out)).map_err(open)?;
            wanted
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                .map_err(open)?;
            wanted
                .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)
                .map_err(open)?;
            reader
                .SetCurrentMediaType(audio, None, &wanted)
                .map_err(open)?;
            if from_hns > 0 {
                let at = PROPVARIANT::from(from_hns);
                reader
                    .SetCurrentPosition(&GUID::zeroed(), &at)
                    .map_err(open)?;
            }
        }
        let duration_ms = unsafe {
            reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|value| u64::try_from(&value).ok())
                .map(|hns| hns / 10_000)
        };
        let mut source = Source {
            reader,
            channels: 0,
            rate: 0,
            duration_ms,
            from_hns,
            ended: false,
        };
        source.read_format()?;
        Ok(source)
    }

    fn read_format(&mut self) -> Result<(), PlayerError> {
        let format = |e: windows::core::Error| PlayerError::Open(e.message());
        let current = unsafe {
            self.reader
                .GetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32)
        }
        .map_err(format)?;
        let channels = unsafe { current.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.map_err(format)?;
        let rate = unsafe { current.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.map_err(format)?;
        if channels == 0 || rate == 0 || (self.rate != 0 && rate != self.rate) {
            return Err(PlayerError::Open(format!(
                "{channels} channels at {rate} Hz"
            )));
        }
        self.channels = channels as usize;
        self.rate = rate;
        Ok(())
    }

    /// The next decoded piece, mixed to stereo, appended to `out`; sets
    /// `ended` at the end of the file.
    fn read(&mut self, out: &mut Vec<f32>) -> Result<(), PlayerError> {
        let read = |e: windows::core::Error| PlayerError::Open(e.message());
        let mut flags = 0u32;
        let mut at = 0i64;
        let mut sample = None;
        unsafe {
            self.reader
                .ReadSample(
                    MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&raw mut flags),
                    Some(&raw mut at),
                    Some(&raw mut sample),
                )
                .map_err(read)?;
        }
        if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            return Err(PlayerError::Open("the source reader failed".into()));
        }
        if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
            self.read_format()?;
        }
        if let Some(sample) = sample {
            let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(read)?;
            let mut data = std::ptr::null_mut();
            let mut len = 0u32;
            unsafe { buffer.Lock(&mut data, None, Some(&raw mut len)) }.map_err(read)?;
            let floats =
                unsafe { std::slice::from_raw_parts(data as *const f32, len as usize / 4) };
            // Frames before the start the seek landed ahead of.
            let early = (self.from_hns - at).max(0) as u128 * u128::from(self.rate) / 10_000_000;
            let skip = (early as usize)
                .saturating_mul(self.channels)
                .min(floats.len());
            downmix(&floats[skip..], self.channels, out);
            let _ = unsafe { buffer.Unlock() };
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            self.ended = true;
        }
        Ok(())
    }
}

/// An owned Win32 event.
struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The shared-mode stream on the default console endpoint: two float
/// channels at the file's rate (Windows converts to the mix format).
struct Output {
    client: IAudioClient,
    render: IAudioRenderClient,
    /// The buffer's size in frames.
    frames: u32,
    /// Last field: closed after the client that signals it is released.
    event: Event,
}

impl Output {
    fn open(rate: u32) -> Result<Output, PlayerError> {
        let dev = |e: windows::core::Error| PlayerError::Device(e.message());
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(dev)?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(dev)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(dev)?;
            let format = WAVEFORMATEX {
                wFormatTag: FORMAT_FLOAT,
                nChannels: 2,
                nSamplesPerSec: rate,
                nAvgBytesPerSec: rate * 8,
                nBlockAlign: 8,
                wBitsPerSample: 32,
                cbSize: 0,
            };
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    BUFFER_HNS,
                    0,
                    &format,
                    None,
                )
                .map_err(dev)?;
            let event = Event(CreateEventW(None, false, false, None).map_err(dev)?);
            client.SetEventHandle(event.0).map_err(dev)?;
            let frames = client.GetBufferSize().map_err(dev)?;
            let render: IAudioRenderClient = client.GetService().map_err(dev)?;
            Ok(Output {
                client,
                render,
                frames,
                event,
            })
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_puts_both_channels_in_both_ears() {
        let mut out = Vec::new();
        downmix(&[1.0, 0.0, 0.0, 1.0], 2, &mut out);
        assert_eq!(out, [0.5, 0.5, 0.5, 0.5]);
        let mut mono = Vec::new();
        downmix(&[0.2], 1, &mut mono);
        assert_eq!(mono, [0.2, 0.2]);
    }

    #[test]
    fn played_ms_counts_frames_from_the_start() {
        assert_eq!(played_ms(1_000, 48_000, 48_000), 2_000);
        assert_eq!(played_ms(0, 24_000, 48_000), 500);
        assert_eq!(played_ms(700, 0, 44_100), 700);
    }

    /// A 1 s stereo 16-bit WAV: 440 Hz left, 660 Hz right.
    fn write_wav(path: &std::path::Path) {
        let rate = 48_000u32;
        let mut data = Vec::new();
        for i in 0..rate {
            let t = i as f32 / rate as f32;
            for hz in [440.0f32, 660.0] {
                let v = (t * hz * std::f32::consts::TAU).sin() * 0.2;
                data.extend_from_slice(&((v * 32_767.0) as i16).to_le_bytes());
            }
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(path, wav).expect("wav");
    }

    /// Plays on the real default device: run by hand
    /// (`cargo test --locked plays_a_generated_wav -- --ignored`).
    #[test]
    #[ignore]
    fn plays_a_generated_wav() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("player-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let wav = dir.join("tone.wav");
        write_wav(&wav);
        let mut player = Player::new(wav);
        player.play_from(0, HWND::default()).expect("plays");
        assert!(player.playing());
        assert_eq!(player.duration_ms(), Some(1_000));
        thread::sleep(std::time::Duration::from_millis(400));
        let midway = player.position_ms();
        assert!((100..900).contains(&midway), "{midway}");
        thread::sleep(std::time::Duration::from_millis(1_100));
        assert!(!player.playing(), "ended");
        assert!(player.position_ms() >= 950, "{}", player.position_ms());
        // From the middle, then paused: the thread goes, the position stays.
        player.play_from(500, HWND::default()).expect("plays again");
        thread::sleep(std::time::Duration::from_millis(200));
        player.pause();
        let held = player.position_ms();
        assert!((500..1_000).contains(&held), "{held}");
        assert!(!player.playing());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
