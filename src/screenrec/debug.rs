//! `vale --screenrec-test <seconds> [--zoom] [--window [hwnd]] [--region x,y,w,h]
//! [--fps 60] [--quality 1080p] [--software] [--no-audio] [--no-audio-push]` (debug builds only).
//!
//! Records the primary monitor (or a window, or a region) into
//! `%TEMP%\vale-screenrec\`, feeding silence into the audio track. A red
//! marker window, kept out of the capture like the notch will be, sits at the
//! top of the primary monitor while it runs. `--zoom` toggles the zoom every
//! 2 s. Then it reads the MP4 back with Media Foundation and prints what it
//! holds: frames, duration, audio, and whether the marker leaked into it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFAttributes, IMFMediaType, IMFSourceReader, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_AUDIO_STREAM,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MFMediaType_Video,
    MFVideoFormat_RGB32,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetForegroundWindow, GetWindowTextW, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW,
    SW_SHOWNOACTIVATE, SetCursorPos, ShowWindow, TranslateMessage, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{HSTRING, Interface, PCWSTR, w};

use super::cursor_log::{self, Entry};
use super::files;
use super::capture::exclude_from_capture;
use super::{Options, ScreenRecorder, Target};
use crate::mf::created;

/// The marker's place on screen: top centre of the primary monitor.
const MARKER_W: i32 = 240;
const MARKER_H: i32 = 36;

struct Args {
    seconds: f64,
    zoom: bool,
    target: Target,
    fps: u32,
    /// Shorter-side cap from `--quality`; `None` = original size.
    max_short: Option<u32>,
    software: bool,
    audio: bool,
    /// The audio track is made but nothing is ever pushed into it (the
    /// sound that could not start): the video must still finish and play.
    audio_push: bool,
    /// Control run: the marker is not excluded, so the check must see it.
    leak_marker: bool,
    /// Moves the real cursor in a circle (put back afterwards).
    wiggle: bool,
    /// Pauses from 40 % to 60 % of the time.
    pause: bool,
}

/// The primary monitor: what a run records unless told otherwise.
fn primary_monitor() -> Target {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
    let m = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    Target::Monitor(m.0 as isize)
}

fn parse(args: &[String]) -> Result<Args, String> {
    let seconds: f64 = args
        .first()
        .and_then(|s| s.parse().ok())
        .filter(|s: &f64| *s > 0.0 && *s <= 3600.0)
        .ok_or("usage: --screenrec-test <seconds> [--zoom] [--window [hwnd]] [--region x,y,w,h] [--fps 60] [--software] [--no-audio]")?;
    let mut a = Args {
        seconds,
        zoom: false,
        target: primary_monitor(),
        fps: 30,
        max_short: None,
        software: false,
        audio: true,
        audio_push: true,
        leak_marker: false,
        wiggle: false,
        pause: false,
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--zoom" => a.zoom = true,
            "--software" => a.software = true,
            "--no-audio" => a.audio = false,
            "--no-audio-push" => a.audio_push = false,
            "--leak-marker" => a.leak_marker = true,
            "--wiggle" => a.wiggle = true,
            "--pause" => a.pause = true,
            "--fps" => {
                i += 1;
                a.fps = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--fps needs 30 or 60")?;
            }
            "--quality" => {
                i += 1;
                a.max_short = match args.get(i).map(String::as_str) {
                    Some("original") => None,
                    Some("1440p") => Some(1440),
                    Some("1080p") => Some(1080),
                    Some("720p") => Some(720),
                    _ => return Err("--quality needs original, 1440p, 1080p or 720p".into()),
                };
            }
            "--window" => {
                let given = args.get(i + 1).and_then(|s| {
                    let s = s.trim_start_matches("0x");
                    isize::from_str_radix(s, 16).ok()
                });
                let hwnd = match given {
                    Some(h) => {
                        i += 1;
                        h
                    }
                    None => unsafe { GetForegroundWindow() }.0 as isize,
                };
                a.target = Target::Window(hwnd);
            }
            "--region" => {
                i += 1;
                let n: Vec<i32> = args
                    .get(i)
                    .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
                    .unwrap_or_default();
                let [x, y, w, h] = n[..] else {
                    return Err("--region needs x,y,w,h".into());
                };
                a.target = Target::Region { x, y, w, h };
            }
            other => return Err(format!("unknown option {other}")),
        }
        i += 1;
    }
    Ok(a)
}

unsafe extern "system" fn marker_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

fn primary_rect() -> (i32, i32, i32, i32) {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let m = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let _ = GetMonitorInfoW(m, &mut info);
    }
    let r = info.rcMonitor;
    (r.left, r.top, r.right - r.left, r.bottom - r.top)
}

/// A pure red notch-sized window at the top of the primary monitor, kept
/// out of every capture. Returns it and whether the exclusion took.
fn show_marker(exclude: bool) -> Option<(HWND, bool)> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = w!("ValeScreenrecMarker");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(marker_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_00FF)),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let (x, y, w, _) = primary_rect();
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("marker"),
            WS_POPUP,
            x + (w - MARKER_W) / 2,
            y,
            MARKER_W,
            MARKER_H,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        let excluded = exclude && exclude_from_capture(hwnd, true);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Some((hwnd, excluded))
    }
}

fn pump() {
    let mut msg = MSG::default();
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Where the cursor was and the primary monitor's centre, when wiggling.
fn wiggle_start(on: bool) -> Option<((i32, i32), (i32, i32))> {
    if !on {
        return None;
    }
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) }.ok()?;
    let (x, y, w, h) = primary_rect();
    Some(((p.x, p.y), (x + w / 2, y + h / 2)))
}

fn window_title(hwnd: isize) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetWindowTextW(HWND(hwnd as _), &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

pub fn run(args: &[String]) {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let dir = std::env::temp_dir().join("vale-screenrec");
    let stem =
        crate::recorder::naming::file_stem(chrono::Local::now().naive_local(), "screenrec-test");
    let stem = files::unique_stem(&dir, &stem, |p| p.exists());
    let path = files::files_of(&dir, &stem)[0].clone();
    match a.target {
        Target::Window(h) => println!("target: window 0x{h:X} \"{}\"", window_title(h)),
        t => println!("target: {t:?}"),
    }
    let marker = show_marker(!a.leak_marker);
    match marker {
        Some((_, true)) => println!("marker: shown, excluded from capture"),
        Some((_, false)) => println!("marker: shown, NOT excluded"),
        None => println!("marker: could not be created"),
    }
    pump();

    let options = Options {
        fps: a.fps,
        max_short: a.max_short,
        audio_channels: a.audio.then_some(2),
        channel_names: vec!["mic".into(), "system".into()],
        prefer_hardware: !a.software,
        title: "screenrec-test".into(),
        ..Options::default()
    };
    let rec = match ScreenRecorder::start(a.target, &path, options, None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("could not start: {e}");
            std::process::exit(1);
        }
    };
    println!("recording {:.1} s to {}", a.seconds, path.display());

    // Track A's stand-in: 10 ms of silence every 10 ms, stamped by QPC.
    let feeding = Arc::new(AtomicBool::new(true));
    let feeder = rec.audio_input().filter(|_| a.audio_push).map(|input| {
        let on = feeding.clone();
        std::thread::spawn(move || {
            let chunk = vec![0.0f32; 480 * 2];
            let mut at = crate::recorder::capture::qpc_hns();
            while on.load(Ordering::Relaxed) {
                input.push_audio(&chunk, 2, 48_000, at);
                at += 100_000;
                let wait = at - crate::recorder::capture::qpc_hns();
                if wait > 0 {
                    std::thread::sleep(Duration::from_micros((wait / 10) as u64));
                }
            }
        })
    });

    let start = Instant::now();
    let mut next_toggle = Duration::from_secs(2);
    let mut toggles = 0u32;
    let home = wiggle_start(a.wiggle);
    let (pause_at, resume_at) = (a.seconds * 0.4, a.seconds * 0.6);
    let mut paused = false;
    while start.elapsed().as_secs_f64() < a.seconds {
        pump();
        let t = start.elapsed().as_secs_f64();
        if a.zoom && start.elapsed() >= next_toggle {
            rec.toggle_zoom();
            toggles += 1;
            // The second zoom-in goes one step further (2.5x).
            if toggles == 2 {
                rec.zoom_step(1);
            }
            next_toggle += Duration::from_secs(2);
        }
        if a.pause && !paused && (pause_at..resume_at).contains(&t) {
            rec.pause();
            paused = true;
            println!("paused at {t:.2} s");
        }
        if a.pause && paused && t >= resume_at {
            rec.resume();
            paused = false;
            println!("resumed at {t:.2} s");
        }
        if let Some((cx, cy)) = home.map(|h| h.1) {
            // A circle every 3 s around the primary monitor's centre.
            let angle = t * std::f64::consts::TAU / 3.0;
            unsafe {
                let _ = SetCursorPos(
                    cx + (300.0 * angle.cos()) as i32,
                    cy + (200.0 * angle.sin()) as i32,
                );
            }
        }
        if !rec.status().running {
            println!("the recording ended by itself");
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(((x, y), _)) = home {
        unsafe {
            let _ = SetCursorPos(x, y);
        }
    }
    let status = rec.status();
    println!(
        "status at stop: zoomed {}, level {}",
        status.zoomed, status.zoom_level
    );
    feeding.store(false, Ordering::Relaxed);
    let result = rec.stop();
    if let Some(f) = feeder {
        let _ = f.join();
    }
    if let Some((hwnd, _)) = marker {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    let summary = match result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("recording failed: {e}");
            std::process::exit(1);
        }
    };
    println!("written: {}", summary.path.display());
    println!(
        "video: {}x{} @ {} fps, {:.2} s, {} frames, {:.2} fps achieved, {} dropped",
        summary.size.0,
        summary.size.1,
        summary.fps,
        summary.duration_ms as f64 / 1000.0,
        summary.frames,
        summary.fps_achieved(),
        summary.dropped
    );
    println!(
        "encoder: {} ({})",
        if summary.encoder.hardware {
            "hardware"
        } else {
            "software"
        },
        summary.encoder.name
    );
    println!("container: {:?}", summary.container);
    println!(
        "ended: {:?}, audio frames dropped: {}, audio chunks refused: {}",
        summary.ended, summary.audio_dropped, summary.audio_rejected
    );
    if let Some(log) = &summary.cursor_log {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        let mut lines = text.lines();
        let header = lines.next().and_then(cursor_log::parse_header);
        let (mut moves, mut clicks, mut zooms, mut other) = (0, 0, 0, 0);
        for entry in lines.map(cursor_log::parse_line) {
            match entry {
                Some(Entry::Move { .. }) => moves += 1,
                Some(Entry::Down { .. }) => clicks += 1,
                Some(Entry::Zoom { .. }) => zooms += 1,
                Some(Entry::Up { .. }) => {}
                None => other += 1,
            }
        }
        println!(
            "cursor log: {} (header {}, {moves} moves, {clicks} clicks, {zooms} zoom changes, {other} unreadable)",
            log.display(),
            if header.is_some() { "ok" } else { "MISSING" }
        );
    }
    for f in files::existing_files(&summary.path) {
        println!("file: {}", f.display());
    }
    if let Ok(meta) = std::fs::metadata(&summary.path) {
        println!("size: {} bytes", meta.len());
    }

    // Read it back.
    let marker_at = matches!(a.target, Target::Monitor(_)).then(|| {
        let (_, _, w, _) = primary_rect();
        let sx = summary.size.0 as f64 / w as f64;
        (
            ((w - MARKER_W) / 2) as f64 * sx,
            0.0,
            MARKER_W as f64 * sx,
            MARKER_H as f64 * sx,
        )
    });
    if let Err(e) = verify(&summary.path, marker_at) {
        eprintln!("read-back failed: {e}");
        std::process::exit(1);
    }
}

/// The samples of one stream (compressed, not decoded) and where they end.
fn count(reader: &IMFSourceReader, stream: u32) -> (u64, i64) {
    let (mut n, mut end) = (0u64, 0i64);
    loop {
        let (mut flags, mut ts) = (0u32, 0i64);
        let mut sample = None;
        let read = unsafe {
            reader.ReadSample(
                stream,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )
        };
        if read.is_err() {
            break;
        }
        if let Some(sample) = sample {
            n += 1;
            end = end.max(ts + unsafe { sample.GetSampleDuration() }.unwrap_or(0));
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
    }
    (n, end)
}

fn source_reader(
    mf: &crate::mf::Mf,
    url: &HSTRING,
    attributes: Option<&IMFAttributes>,
) -> windows::core::Result<IMFSourceReader> {
    let attrs = attributes.map_or(std::ptr::null_mut(), |a| a.as_raw());
    created(|p| unsafe { (mf.create_source_reader)(PCWSTR(url.as_ptr()), attrs, p) })
}

/// Counts the MP4's samples per stream and checks the marker's place in
/// its first frame.
fn verify(
    path: &std::path::Path,
    marker: Option<(f64, f64, f64, f64)>,
) -> windows::core::Result<()> {
    let session = crate::mf::MfSession::start().ok_or_else(windows::core::Error::empty)?;
    let mf = session.api();
    let url = HSTRING::from(path);
    let reader = source_reader(mf, &url, None)?;
    let (video, v_end) = count(&reader, MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32);
    let reader = source_reader(mf, &url, None)?;
    let (audio, a_end) = count(&reader, MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32);
    println!(
        "read back: {video} video frames ending at {:.2} s, {audio} audio packets ending at {:.2} s",
        v_end as f64 / 1e7,
        a_end as f64 / 1e7
    );

    // Decode a frame every second (from 0.5 s), save them next to the
    // video as PNG, and look for the marker in the first.
    let attrs: IMFAttributes = created(|p| unsafe { (mf.create_attributes)(p, 1) })?;
    unsafe { attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)? };
    let reader = source_reader(mf, &url, Some(&attrs))?;
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let rgb: IMFMediaType = created(|p| unsafe { (mf.create_media_type)(p) })?;
    unsafe {
        rgb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        rgb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(stream, None, &rgb)?;
    }
    let t = unsafe { reader.GetCurrentMediaType(stream)? };
    let size = unsafe { t.GetUINT64(&MF_MT_FRAME_SIZE)? };
    let (w, h) = ((size >> 32) as usize, (size & 0xFFFF_FFFF) as usize);
    let stride =
        unsafe { t.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.map_or(w as i64 * 4, |s| s as i32 as i64);
    let mut want = 5_000_000i64;
    let mut first = true;
    loop {
        let (mut flags, mut ts) = (0u32, 0i64);
        let mut sample = None;
        unsafe {
            reader.ReadSample(
                stream,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )?
        };
        let Some(sample) = sample else { break };
        if ts + 1 < want {
            continue;
        }
        want += 10_000_000;
        // Top-down BGRA, opaque.
        let mut px = vec![0u8; w * h * 4];
        // The decoder's rows can be wider than the picture: Lock2D gives
        // the real pitch (negative: bottom-up).
        let buffer = unsafe { sample.GetBufferByIndex(0)? };
        unsafe {
            if let Ok(b2) = buffer.cast::<IMF2DBuffer>() {
                let mut scan0 = std::ptr::null_mut();
                let mut pitch = 0i32;
                b2.Lock2D(&mut scan0, &mut pitch)?;
                for y in 0..h {
                    let row = scan0.offset(y as isize * pitch as isize);
                    let src = std::slice::from_raw_parts(row, w * 4);
                    px[y * w * 4..(y + 1) * w * 4].copy_from_slice(src);
                }
                b2.Unlock2D()?;
            } else {
                let mut data = std::ptr::null_mut();
                let mut len = 0u32;
                buffer.Lock(&mut data, None, Some(&mut len))?;
                let bytes = std::slice::from_raw_parts(data, len as usize);
                for y in 0..h {
                    let row = if stride < 0 { h - 1 - y } else { y };
                    let from = row * stride.unsigned_abs() as usize;
                    if let Some(src) = bytes.get(from..from + w * 4) {
                        px[y * w * 4..(y + 1) * w * 4].copy_from_slice(src);
                    }
                }
                buffer.Unlock()?;
            }
        }
        for p in px.chunks_exact_mut(4) {
            p[3] = 255;
        }
        let png = path.with_extension(format!("frame-{:05}ms.png", ts / 10_000));
        if let Some(bytes) = crate::screenshot::win::encode_png(w as u32, h as u32, &px)
            && std::fs::write(&png, bytes).is_ok()
        {
            println!("frame: {}", png.display());
        }
        if std::mem::take(&mut first)
            && let Some((mx, my, mw, mh)) = marker
        {
            let (mut red, mut total) = (0u64, 0u64);
            for y in (my as usize)..((my + mh) as usize).min(h) {
                for x in (mx as usize)..((mx + mw) as usize).min(w) {
                    let p = &px[(y * w + x) * 4..(y * w + x) * 4 + 4];
                    total += 1;
                    if p[2] > 200 && p[1] < 60 && p[0] < 60 {
                        red += 1;
                    }
                }
            }
            println!(
                "marker check: {red} of {total} pixels in the marker's place are marker red ({})",
                if red * 10 < total.max(1) {
                    "kept out"
                } else {
                    "LEAKED"
                }
            );
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
    }
    Ok(())
}
