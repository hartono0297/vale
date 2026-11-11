//! Screen recording (track C core): Windows Graphics Capture, a D3D11
//! compositor for the zoom and the cursor, and a Media Foundation sink
//! writer to MP4. Nothing here runs, and no thread, capture or timer exists,
//! until [`ScreenRecorder::start`]; everything ends with the recording.

// Not wired into the app until track A lands; the debug CLI
// (`--screenrec-test`) is the only caller in a debug build.
#![cfg_attr(not(debug_assertions), allow(dead_code, unused_imports))]

pub mod audio;
pub mod capture;
pub mod compose;
pub mod cursor_log;
#[cfg(debug_assertions)]
pub mod debug;
pub mod encode;
pub mod exclude;
pub mod files;
pub mod plan;
pub mod session;
// Nothing calls it until the stop thread does.
#[cfg_attr(not(test), allow(dead_code))]
pub mod thumb;
pub mod watch;
pub mod zoom;

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};

pub use crate::recorder::Container;
pub use audio::AudioInput;
pub use capture::{Target, exclude_from_capture};
pub use encode::EncoderInfo;

use audio::{AudioClock, AudioFeed};
use capture::{Capture, Source};
use compose::{Compositor, Gpu, Overlay};
use cursor_log::{Button, CursorLog, Entry, Header};
use encode::{Encoder, VideoSpec};
use plan::Timeline;
use zoom::ZoomCamera;

use crate::mf::{MfSession, created};
use crate::recorder::capture::qpc_hns;

/// Why a recording could not start or went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Media Foundation is missing: a Windows "N" edition without the Media
    /// Feature Pack.
    NoMediaFoundation,
    /// This Windows cannot capture the screen (Graphics Capture missing).
    CaptureUnsupported,
    /// The monitor or window to record is gone.
    TargetGone,
    /// The window to record is minimised (it has no picture to capture);
    /// restore it first.
    TargetMinimized,
    /// No H.264 encoder took the video (the detail is Windows' message).
    NoEncoder(String),
    /// No usable Direct3D 11 device or video processor.
    Gpu(String),
    /// A Windows call failed.
    Windows {
        step: &'static str,
        code: i32,
        message: String,
    },
    /// The video folder or a side file could not be written.
    Io(String),
}

impl Error {
    fn win(step: &'static str, e: &windows::core::Error) -> Error {
        Error::Windows {
            step,
            code: e.code().0,
            message: e.message(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoMediaFoundation => write!(
                f,
                "Media Foundation is not installed (Windows N: install the Media Feature Pack)"
            ),
            Error::CaptureUnsupported => write!(f, "this Windows cannot capture the screen"),
            Error::TargetGone => write!(f, "the window or monitor to record is gone"),
            Error::TargetMinimized => write!(f, "the window to record is minimised"),
            Error::NoEncoder(m) => write!(f, "no H.264 encoder took the video: {m}"),
            Error::Gpu(m) => write!(f, "Direct3D: {m}"),
            Error::Windows {
                step,
                code,
                message,
            } => write!(f, "{step} failed (0x{code:08X}): {message}"),
            Error::Io(m) => write!(f, "{m}"),
        }
    }
}

/// How to record.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// 30 (default) or 60.
    pub fps: u32,
    /// Scale the video down to this height (aspect kept); `None` = the
    /// source's own size (capped at 4096 x 2304).
    pub max_height: Option<u32>,
    /// Add an AAC track with 1 or 2 channels, fed through
    /// [`ScreenRecorder::audio_input`].
    pub audio_channels: Option<u16>,
    /// The sidecar's `"channels"` (index = channel), e.g. `["mic", "system"]`.
    pub channel_names: Vec<String>,
    pub draw_cursor: bool,
    pub click_ripples: bool,
    /// Try a hardware encoder first (the software one is the fallback).
    pub prefer_hardware: bool,
    /// Write `<name>.cursor.jsonl` and the `.vale.json` sidecar.
    pub side_files: bool,
    pub title: String,
    pub app: Option<String>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            fps: 30,
            max_height: None,
            audio_channels: None,
            channel_names: Vec::new(),
            draw_cursor: true,
            click_ripples: true,
            prefer_hardware: true,
            side_files: true,
            title: String::new(),
            app: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// [`ScreenRecorder::stop`].
    Stopped,
    /// The window was closed or the monitor went away.
    TargetClosed,
}

/// A finished recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub path: PathBuf,
    pub cursor_log: Option<PathBuf>,
    pub size: (u32, u32),
    pub fps: u32,
    pub duration_ms: u64,
    /// Frames written.
    pub frames: u64,
    /// Frame slots skipped (the machine fell behind or the encoder was full).
    pub dropped: u64,
    pub encoder: EncoderInfo,
    /// The MP4 flavour written (fragmented unless the sink writer refused it).
    pub container: Container,
    pub ended: EndReason,
    /// Audio frames dropped (queue overflow, captured inside a pause).
    pub audio_dropped: u64,
    /// Audio chunks refused (not 48 kHz).
    pub audio_rejected: u64,
}

impl Summary {
    pub fn fps_achieved(&self) -> f64 {
        if self.duration_ms == 0 {
            0.0
        } else {
            self.frames as f64 * 1000.0 / self.duration_ms as f64
        }
    }
}

/// What the notch shows while recording.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    pub elapsed_ms: u64,
    pub paused: bool,
    pub zoomed: bool,
    /// The level used while zoomed (shown even while not zoomed).
    pub zoom_level: f64,
    pub frames: u64,
    pub dropped: u64,
    /// The recording thread is still running.
    pub running: bool,
}

enum Cmd {
    Pause,
    Resume,
    ToggleZoom,
    ZoomStep(i32),
    Stop,
}

/// A screen recording in progress, on its own thread.
pub struct ScreenRecorder {
    tx: Sender<Cmd>,
    thread: Option<JoinHandle<Result<Summary, Error>>>,
    feed: Option<Arc<AudioFeed>>,
    status: Arc<Mutex<Status>>,
}

impl ScreenRecorder {
    /// Starts recording `target` into `path` (the final `.mp4`; its folder is
    /// made). The video is written to `path.part` and renamed to `path` when
    /// the recording ends. Returns once the capture and the encoder run, or with the
    /// reason they could not. `on_end` is called on the recording thread
    /// when the recording ends by itself (the target closed, an error);
    /// call [`stop`](Self::stop) then to collect the result.
    pub fn start(
        target: Target,
        path: &Path,
        options: Options,
        on_end: Option<Box<dyn Fn() + Send>>,
    ) -> Result<ScreenRecorder, Error> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        }
        let feed = options.audio_channels.map(|c| Arc::new(AudioFeed::new(c)));
        let status = Arc::new(Mutex::new(Status {
            zoom_level: zoom::DEFAULT_LEVEL,
            running: true,
            ..Status::default()
        }));
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let path = path.to_path_buf();
        let thread_feed = feed.clone();
        let thread_status = status.clone();
        let thread = std::thread::Builder::new()
            .name("vale-screenrec".into())
            .spawn(move || {
                let com = ComGuard::new();
                let result = match Recording::prepare(target, &path, &options, thread_feed) {
                    Ok(rec) => {
                        let _ = ready_tx.send(Ok(()));
                        rec.run(rx, &thread_status, on_end)
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.clone()));
                        Err(e)
                    }
                };
                if let Ok(mut s) = thread_status.lock() {
                    s.running = false;
                }
                drop(com);
                result
            })
            .map_err(|e| Error::Io(format!("thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(ScreenRecorder {
                tx,
                thread: Some(thread),
                feed,
                status,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(thread
                .join()
                .ok()
                .and_then(|r| r.err())
                .unwrap_or(Error::Io("the recording thread ended".into()))),
        }
    }

    /// Where track A's capture pushes audio (`None` without an audio track).
    pub fn audio_input(&self) -> Option<Arc<dyn AudioInput>> {
        self.feed.clone().map(|f| f as Arc<dyn AudioInput>)
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }

    pub fn resume(&self) {
        let _ = self.tx.send(Cmd::Resume);
    }

    /// Zoom in to the cursor, or back out (in the video only).
    pub fn toggle_zoom(&self) {
        let _ = self.tx.send(Cmd::ToggleZoom);
    }

    /// One zoom level up (`dir > 0`) or down, 1x to 4x in 0.5 steps.
    pub fn zoom_step(&self, dir: i32) {
        let _ = self.tx.send(Cmd::ZoomStep(dir));
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| *s).unwrap_or_default()
    }

    /// Ends the recording and finishes the files.
    pub fn stop(mut self) -> Result<Summary, Error> {
        self.finish()
    }

    fn finish(&mut self) -> Result<Summary, Error> {
        let _ = self.tx.send(Cmd::Stop);
        match self.thread.take() {
            Some(t) => t
                .join()
                .unwrap_or_else(|_| Err(Error::Io("the recording thread panicked".into()))),
            None => Err(Error::Io("already stopped".into())),
        }
    }
}

impl Drop for ScreenRecorder {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.finish();
        }
    }
}

struct ComGuard(bool);

impl ComGuard {
    fn new() -> ComGuard {
        ComGuard(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// Mouse buttons as polled each frame.
const BUTTONS: [(
    Button,
    windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
); 3] = [
    (Button::Left, VK_LBUTTON),
    (Button::Right, VK_RBUTTON),
    (Button::Middle, VK_MBUTTON),
];

/// The audio side of the recording loop.
#[derive(Default)]
struct AudioState {
    clock: AudioClock,
    /// Where the last written audio ends (100 ns).
    end: i64,
    /// Frames dropped by the timeline (inside or cut at a pause).
    dropped: u64,
}

/// Everything one recording holds, on its thread. Field order is drop
/// order: the encoder before Media Foundation shuts down.
struct Recording {
    /// The finished video; the encoder writes `files::part_of(path)`.
    path: PathBuf,
    options: Options,
    started: String,
    feed: Option<Arc<AudioFeed>>,
    log: Option<(PathBuf, CursorLog<BufWriter<File>>)>,
    camera: ZoomCamera,
    out: (u32, u32),
    encoder: Encoder,
    compositor: Compositor,
    capture: Capture,
    source: Source,
    _manager: IMFDXGIDeviceManager,
    _gpu: Gpu,
    _mf: MfSession,
}

impl Recording {
    fn prepare(
        target: Target,
        path: &Path,
        options: &Options,
        feed: Option<Arc<AudioFeed>>,
    ) -> Result<Recording, Error> {
        let session = MfSession::start().ok_or(Error::NoMediaFoundation)?;
        let mf = session.api();
        let source = Source::resolve(target)?;
        let gpu = Gpu::new()?;
        let fps = if options.fps >= 60 { 60 } else { 30 };
        let mut capture = Capture::start(&source, &gpu.winrt, fps)?;

        // The first frame gives the real size; a minimised window has none
        // yet and starts black at the item's size.
        let mut first = None;
        for _ in 0..200 {
            if let Some(f) = capture.latest()? {
                first = Some(f);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let src = first
            .as_ref()
            .map_or_else(|| source.item_size(), |f| f.content);
        let src = (src.0.max(2), src.1.max(2));
        let bounds = source.bounds(src.0, src.1);
        let out = plan::output_size(bounds.w as u32, bounds.h as u32, options.max_height);
        let compositor = Compositor::new(&gpu, src, out, fps)?;
        if let Some(f) = &first {
            compositor.take_frame(&f.texture);
        }
        drop(first);

        let mut token = 0u32;
        let manager: IMFDXGIDeviceManager =
            created(|p| unsafe { (mf.create_dxgi_device_manager)(&mut token, p) })
                .map_err(|e| Error::win("MFCreateDXGIDeviceManager", &e))?;
        unsafe { manager.ResetDevice(&gpu.device, token) }
            .map_err(|e| Error::win("ResetDevice", &e))?;
        let spec = VideoSpec {
            width: out.0,
            height: out.1,
            fps,
            bitrate: plan::bitrate(out.0, out.1, fps),
        };
        let channels = feed.as_ref().map(|f| f.channels());
        // Fragmented MP4 first (a crash leaves a playable file), plain MP4
        // when the sink writer refuses it.
        let part = files::part_of(path);
        let mut encoder = None;
        let mut last = Error::NoEncoder(String::new());
        for container in [Container::Fragmented, Container::Mpeg4] {
            match Encoder::create(
                mf,
                &part,
                &manager,
                &spec,
                channels,
                options.prefer_hardware,
                container,
            ) {
                Ok(e) => {
                    encoder = Some(e);
                    break;
                }
                Err(e) => last = e,
            }
        }
        let mut encoder = encoder.ok_or(last)?;
        encoder.name_adapter(&gpu.adapter);

        let log = if options.side_files {
            let log_path = files::cursor_log_path(path);
            let header = Header {
                fps,
                source: (bounds.w as u32, bounds.h as u32),
                video: out,
            };
            File::create(&log_path)
                .and_then(|f| CursorLog::new(BufWriter::new(f), &header))
                .ok()
                .map(|l| (log_path, l))
        } else {
            None
        };
        let mut options = options.clone();
        options.fps = fps;
        Ok(Recording {
            path: path.to_path_buf(),
            options,
            started: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            feed,
            log,
            camera: ZoomCamera::new(bounds, out.0, out.1),
            out,
            encoder,
            compositor,
            capture,
            source,
            _manager: manager,
            _gpu: gpu,
            _mf: session,
        })
    }

    fn log(&mut self, e: Entry) {
        if let Some((_, log)) = &mut self.log
            && log.record(e).is_err()
        {
            // A full disk: the video matters more than the log.
            self.log = None;
        }
    }

    /// Copies the newest captured frame, following a size change.
    fn take_capture(&mut self) -> Result<(), Error> {
        let Some(frame) = self.capture.latest()? else {
            return Ok(());
        };
        if frame.content != self.compositor.source_size() {
            self.compositor.resize_source(frame.content)?;
            self.camera
                .set_bounds(self.source.bounds(frame.content.0, frame.content.1));
        }
        self.compositor.take_frame(&frame.texture);
        Ok(())
    }

    /// Drains the audio feed and writes what the timeline keeps: a chunk
    /// whose start is inside a pause is dropped, one that runs into the next
    /// pause is cut at it. Runs also while paused: the last moment before a
    /// pause arrives late and still lands.
    fn write_audio(&mut self, timeline: &Timeline, st: &mut AudioState) -> Result<(), Error> {
        let Some(feed) = self.feed.clone() else {
            return Ok(());
        };
        let channels = feed.channels() as usize;
        for mut chunk in feed.drain() {
            let n = (chunk.pcm.len() / channels) as u64;
            let Some(measured) = timeline.media_time(chunk.qpc) else {
                st.dropped += n;
                continue;
            };
            let keep = timeline.frames_before_pause(chunk.qpc, n, audio::SAMPLE_RATE);
            st.dropped += n - keep;
            if keep == 0 {
                continue;
            }
            chunk.pcm.truncate(keep as usize * channels);
            let at = st.clock.place(measured, keep);
            self.encoder.write_audio(&chunk.pcm, at)?;
            st.end = st.end.max(at + audio::frames_hns(keep));
        }
        Ok(())
    }

    fn run(
        mut self,
        rx: Receiver<Cmd>,
        status: &Mutex<Status>,
        on_end: Option<Box<dyn Fn() + Send>>,
    ) -> Result<Summary, Error> {
        let fps = self.options.fps;
        let frame_hns = 10_000_000 / fps as i64;
        let mut timeline = Timeline::new(qpc_hns(), 10_000_000);
        let mut next: i64 = 0;
        let mut frames: u64 = 0;
        let mut dropped: u64 = 0;
        let mut audio = AudioState::default();
        let mut ended = EndReason::Stopped;
        let mut failure: Option<Error> = None;
        let mut by_command = false;
        let mut toggles = 0u32;
        let mut steps = 0i32;
        let mut held = [false; 3];
        let mut ripples: Vec<((f32, f32), f64)> = Vec::new();
        let mut cursor = self.camera.bounds().center();
        let mut zoom_logged = 0.0f64;

        'run: loop {
            loop {
                match rx.try_recv() {
                    Ok(Cmd::Pause) => timeline.pause(qpc_hns()),
                    Ok(Cmd::Resume) => {
                        if timeline.is_paused() {
                            timeline.resume(qpc_hns());
                            audio.clock.reset();
                        }
                    }
                    Ok(Cmd::ToggleZoom) => toggles += 1,
                    Ok(Cmd::ZoomStep(d)) => steps += d.signum(),
                    Ok(Cmd::Stop) | Err(TryRecvError::Disconnected) => {
                        by_command = true;
                        break 'run;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            if self.capture.is_closed() || self.source.is_gone() {
                ended = EndReason::TargetClosed;
                break;
            }

            // Audio, also while paused: the last moment before a pause
            // arrives late and still lands; what was captured inside a
            // pause is dropped.
            if let Err(e) = self.write_audio(&timeline, &mut audio) {
                failure = Some(e);
                break;
            }

            if timeline.is_paused() {
                if let Ok(mut s) = status.lock() {
                    s.paused = true;
                }
                // Keep the newest picture so the resume starts from now.
                if let Err(e) = self.take_capture() {
                    failure = Some(e);
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let now = qpc_hns();
            let media = timeline.media_time(now).unwrap_or(0);
            let due = media / frame_hns;
            if due < next {
                let wait = next * frame_hns - media;
                std::thread::sleep(Duration::from_micros((wait / 10).max(500) as u64));
                continue;
            }
            dropped += (due - next) as u64;
            let time = due * frame_hns;
            let t_ms = time as f64 / 10_000.0;
            let t_log = (time / 10_000) as u64;

            if let Err(e) = self.take_capture() {
                failure = Some(e);
                break;
            }

            // The cursor, in source pixels.
            let origin = self.source.screen_origin();
            let bounds = self.camera.bounds();
            let now_cursor = compose::cursor_now();
            if let Some(((x, y), _)) = now_cursor {
                cursor = ((x - origin.0) as f64, (y - origin.1) as f64);
            }
            let rel = (
                (cursor.0 - bounds.x).round() as i32,
                (cursor.1 - bounds.y).round() as i32,
            );
            self.log(Entry::Move {
                t: t_log,
                x: rel.0,
                y: rel.1,
            });
            for (i, (button, vk)) in BUTTONS.iter().enumerate() {
                let down = unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000 != 0;
                if down != held[i] {
                    held[i] = down;
                    let (x, y) = rel;
                    self.log(if down {
                        Entry::Down {
                            t: t_log,
                            x,
                            y,
                            button: *button,
                        }
                    } else {
                        Entry::Up {
                            t: t_log,
                            x,
                            y,
                            button: *button,
                        }
                    });
                    if down && self.options.click_ripples {
                        ripples.push(((cursor.0 as f32, cursor.1 as f32), t_ms));
                    }
                }
            }
            ripples.retain(|(_, start)| compose::ripple_at(t_ms - start, 1.0).is_some());

            // The zoom.
            for _ in 0..std::mem::take(&mut toggles) {
                self.camera.toggle(t_ms, cursor);
            }
            if steps != 0 {
                for _ in 0..steps.unsigned_abs() {
                    self.camera.step(steps.signum(), t_ms);
                }
                steps = 0;
            }
            let zoom_now = if self.camera.is_on() {
                self.camera.level()
            } else {
                0.0
            };
            if zoom_now != zoom_logged {
                zoom_logged = zoom_now;
                self.log(Entry::Zoom {
                    t: t_log,
                    level: zoom_now,
                });
            }
            let view = self.camera.update(t_ms, cursor);

            // Compose and encode.
            let aged: Vec<((f32, f32), f64)> = ripples
                .iter()
                .map(|&(at, start)| (at, t_ms - start))
                .collect();
            let overlay = Overlay {
                cursor: if self.options.draw_cursor {
                    now_cursor.map(|(_, h)| ((cursor.0 as f32, cursor.1 as f32), h))
                } else {
                    None
                },
                ripples: &aged,
            };
            let step = (|| -> Result<bool, Error> {
                let Some((sample, texture, sub)) = self.encoder.next_frame()? else {
                    return Ok(false);
                };
                self.compositor.draw_overlay(&overlay)?;
                let mapped = plan::map_view(view, bounds, self.out.0, self.out.1);
                self.compositor.render(mapped, &texture, sub)?;
                self.encoder.write_frame(&sample, time)?;
                Ok(true)
            })();
            match step {
                Ok(true) => frames += 1,
                Ok(false) => dropped += 1,
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
            next = due + 1;

            // No audio for a while: let the muxer go on without it.
            if self.encoder.has_audio() && time - audio.end > 10_000_000 {
                self.encoder.audio_gap(time);
                audio.end = time;
            }

            if let Ok(mut s) = status.lock() {
                s.elapsed_ms = t_log;
                s.paused = false;
                s.zoomed = self.camera.is_on();
                s.zoom_level = self.camera.level();
                s.frames = frames;
                s.dropped = dropped;
            }
        }

        // What the mix writer flushed just before the end still lands.
        if failure.is_none()
            && let Err(e) = self.write_audio(&timeline, &mut audio)
        {
            failure = Some(e);
        }
        if !by_command && let Some(cb) = &on_end {
            cb();
        }
        let duration_ms = (next * frame_hns / 10_000) as u64;
        let audio_dropped = audio.dropped + self.feed.as_ref().map_or(0, |f| f.dropped_frames());
        let cursor_log = self
            .log
            .take()
            .and_then(|(p, log)| log.finish().ok().map(|_| p));
        let info = self.encoder.info().clone();
        let container = self.encoder.container();
        let Recording {
            path,
            options,
            started,
            encoder,
            ..
        } = self;
        let part = files::part_of(&path);
        let mut finished = encoder.finish();
        if finished.is_ok()
            && let Err(e) = std::fs::rename(&part, &path)
        {
            // The finished video stays as `.part`; nothing else is written.
            finished = Err(Error::Io(format!("{}: {e}", part.display())));
        }
        if finished.is_ok() && options.side_files {
            let sidecar = files::SidecarInfo {
                title: options.title.clone(),
                started,
                duration_ms,
                channels: if options.audio_channels.is_some() {
                    options.channel_names.clone()
                } else {
                    Vec::new()
                },
                app: options.app.clone(),
                cursor_log: cursor_log
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned()),
            };
            let _ = files::write_sidecar(&path, &sidecar);
        }
        if let Some(e) = failure {
            return Err(e);
        }
        finished?;
        Ok(Summary {
            path,
            cursor_log,
            size: self.out,
            fps,
            duration_ms,
            frames,
            dropped,
            encoder: info,
            container,
            ended,
            audio_dropped,
            audio_rejected: self.feed.as_ref().map_or(0, |f| f.rejected_chunks()),
        })
    }
}
