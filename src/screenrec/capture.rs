//! Windows Graphics Capture: a monitor, a window, or a region (a monitor
//! plus a crop), into a free-threaded frame pool the recording thread drains
//! once per video frame. The cursor is left out of the capture
//! (`IsCursorCaptureEnabled(false)`): the compositor draws its own.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromRect,
};
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, IsWindow, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};
use windows::core::{IInspectable, Interface, Ref};

use super::Error;
use super::zoom::RectF;

/// What to record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A whole monitor (`HMONITOR` as a number).
    Monitor(isize),
    /// One window (`HWND` as a number); the video follows it when it moves.
    Window(isize),
    /// A rect in virtual-screen physical pixels, cut from the monitor that
    /// holds most of it.
    Region { x: i32, y: i32, w: i32, h: i32 },
}

impl Target {
    /// The primary monitor.
    pub fn primary_monitor() -> Target {
        let m = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        Target::Monitor(m.0 as isize)
    }
}

/// Keeps `hwnd` out of every screen capture (`WDA_EXCLUDEFROMCAPTURE`), or
/// lets it back in. Vale calls it on its own windows; it works only on
/// windows of this process. `false` when Windows refused.
pub fn exclude_from_capture(hwnd: HWND, exclude: bool) -> bool {
    let affinity = if exclude {
        WDA_EXCLUDEFROMCAPTURE
    } else {
        WDA_NONE
    };
    unsafe { SetWindowDisplayAffinity(hwnd, affinity) }.is_ok()
}

fn monitor_rect(monitor: HMONITOR) -> Option<RECT> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }
        .as_bool()
        .then_some(info.rcMonitor)
}

fn window_frame(hwnd: HWND) -> Option<RECT> {
    let mut frame = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()
    .map(|_| frame)
}

/// `region` (screen pixels) cut to `monitor` and made relative to it.
pub fn crop_in_monitor(region: RECT, monitor: RECT) -> Option<RectF> {
    let left = region.left.max(monitor.left);
    let top = region.top.max(monitor.top);
    let right = region.right.min(monitor.right);
    let bottom = region.bottom.min(monitor.bottom);
    (right - left >= 2 && bottom - top >= 2).then(|| {
        RectF::new(
            (left - monitor.left) as f64,
            (top - monitor.top) as f64,
            (right - left) as f64,
            (bottom - top) as f64,
        )
    })
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Monitor { monitor: isize },
    Window { hwnd: isize },
    Region { monitor: isize, crop: RectF },
}

/// A resolved target: the capture item and how its pixels sit on screen.
pub struct Source {
    pub item: GraphicsCaptureItem,
    kind: Kind,
}

impl Source {
    pub fn resolve(target: Target) -> Result<Source, Error> {
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(Error::CaptureUnsupported);
        }
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(|_| Error::CaptureUnsupported)?;
        match target {
            Target::Monitor(m) => {
                let monitor = HMONITOR(m as _);
                monitor_rect(monitor).ok_or(Error::TargetGone)?;
                let item = unsafe { interop.CreateForMonitor::<GraphicsCaptureItem>(monitor) }
                    .map_err(|e| Error::win("CreateForMonitor", &e))?;
                Ok(Source {
                    item,
                    kind: Kind::Monitor { monitor: m },
                })
            }
            Target::Window(h) => {
                let hwnd = HWND(h as _);
                if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                    return Err(Error::TargetGone);
                }
                if unsafe { IsIconic(hwnd) }.as_bool() {
                    return Err(Error::TargetMinimized);
                }
                let item = unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(hwnd) }
                    .map_err(|e| Error::win("CreateForWindow", &e))?;
                Ok(Source {
                    item,
                    kind: Kind::Window { hwnd: h },
                })
            }
            Target::Region { x, y, w, h } => {
                let region = RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                };
                let monitor = unsafe { MonitorFromRect(&region, MONITOR_DEFAULTTONEAREST) };
                let rect = monitor_rect(monitor).ok_or(Error::TargetGone)?;
                let crop = crop_in_monitor(region, rect).ok_or(Error::TargetGone)?;
                let item = unsafe { interop.CreateForMonitor::<GraphicsCaptureItem>(monitor) }
                    .map_err(|e| Error::win("CreateForMonitor", &e))?;
                Ok(Source {
                    item,
                    kind: Kind::Region {
                        monitor: monitor.0 as isize,
                        crop,
                    },
                })
            }
        }
    }

    /// The item's size now (the whole monitor for a region).
    pub fn item_size(&self) -> (u32, u32) {
        let s = self.item.Size().unwrap_or_default();
        (s.Width.max(0) as u32, s.Height.max(0) as u32)
    }

    /// The part of a `w` x `h` frame that is recorded.
    pub fn bounds(&self, w: u32, h: u32) -> RectF {
        match self.kind {
            Kind::Region { crop, .. } => crop,
            _ => RectF::new(0.0, 0.0, w as f64, h as f64),
        }
    }

    /// Where the frame's top-left pixel is on screen now (a window moves,
    /// monitors can be rearranged).
    pub fn screen_origin(&self) -> (i32, i32) {
        match self.kind {
            Kind::Monitor { monitor } | Kind::Region { monitor, .. } => {
                monitor_rect(HMONITOR(monitor as _)).map_or((0, 0), |r| (r.left, r.top))
            }
            Kind::Window { hwnd } => {
                window_frame(HWND(hwnd as _)).map_or((0, 0), |r| (r.left, r.top))
            }
        }
    }

    /// The window was destroyed or the monitor unplugged. Checked every
    /// frame besides the item's `Closed` event, which does not always come
    /// for a window that closes.
    pub fn is_gone(&self) -> bool {
        match self.kind {
            Kind::Monitor { monitor } | Kind::Region { monitor, .. } => {
                monitor_rect(HMONITOR(monitor as _)).is_none()
            }
            Kind::Window { hwnd } => !unsafe { IsWindow(Some(HWND(hwnd as _))) }.as_bool(),
        }
    }
}

/// A captured frame, held until its pixels are copied.
pub struct Frame {
    _frame: Direct3D11CaptureFrame,
    pub texture: ID3D11Texture2D,
    /// The valid part of the texture.
    pub content: (u32, u32),
}

/// A running capture session.
pub struct Capture {
    item: GraphicsCaptureItem,
    device: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
    closed: Arc<AtomicBool>,
    closed_token: i64,
}

const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;
const BUFFERS: i32 = 2;

impl Capture {
    pub fn start(source: &Source, device: &IDirect3DDevice, fps: u32) -> Result<Capture, Error> {
        let item = source.item.clone();
        let size = item
            .Size()
            .map_err(|e| Error::win("GraphicsCaptureItem::Size", &e))?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(device, FORMAT, BUFFERS, size)
            .map_err(|e| Error::win("CreateFreeThreaded", &e))?;
        let session = pool
            .CreateCaptureSession(&item)
            .map_err(|e| Error::win("CreateCaptureSession", &e))?;
        // Older Windows builds lack these; the capture still works.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        // Windows 11 24H2+: no frames faster than the video needs.
        let _ = session.SetMinUpdateInterval(TimeSpan {
            Duration: 10_000_000 / fps.max(1) as i64,
        });
        let closed = Arc::new(AtomicBool::new(false));
        let flag = closed.clone();
        let closed_token = item
            .Closed(
                &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(
                    move |_: Ref<GraphicsCaptureItem>, _: Ref<IInspectable>| {
                        flag.store(true, Ordering::SeqCst);
                        Ok(())
                    },
                ),
            )
            .map_err(|e| Error::win("GraphicsCaptureItem::Closed", &e))?;
        session
            .StartCapture()
            .map_err(|e| Error::win("StartCapture", &e))?;
        Ok(Capture {
            item,
            device: device.clone(),
            pool,
            session,
            size,
            closed,
            closed_token,
        })
    }

    /// The window or monitor went away.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// The newest frame since the last call, older ones dropped. `None`
    /// when nothing new came, and once when the size changed (the pool is
    /// made again at the new size; the next frame has it).
    pub fn latest(&mut self) -> Result<Option<Frame>, Error> {
        let mut last = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            last = Some(frame);
        }
        let Some(frame) = last else { return Ok(None) };
        let content = frame
            .ContentSize()
            .map_err(|e| Error::win("ContentSize", &e))?;
        if content.Width <= 0 || content.Height <= 0 {
            return Ok(None);
        }
        if content != self.size {
            drop(frame);
            self.size = content;
            self.pool
                .Recreate(&self.device, FORMAT, BUFFERS, content)
                .map_err(|e| Error::win("Recreate", &e))?;
            return Ok(None);
        }
        let surface = frame.Surface().map_err(|e| Error::win("Surface", &e))?;
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|e| Error::win("IDirect3DDxgiInterfaceAccess", &e))?;
        let texture: ID3D11Texture2D =
            unsafe { access.GetInterface() }.map_err(|e| Error::win("GetInterface", &e))?;
        Ok(Some(Frame {
            _frame: frame,
            texture,
            content: (content.Width as u32, content.Height as u32),
        }))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.item.RemoveClosed(self.closed_token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn a_region_is_cut_to_its_monitor() {
        let monitor = r(1920, 0, 3840, 1080);
        let crop = crop_in_monitor(r(1800, 100, 2420, 500), monitor).unwrap();
        assert_eq!(crop, RectF::new(0.0, 100.0, 500.0, 400.0));
        let crop = crop_in_monitor(r(2000, 50, 2100, 150), monitor).unwrap();
        assert_eq!(crop, RectF::new(80.0, 50.0, 100.0, 100.0));
        assert!(crop_in_monitor(r(0, 0, 100, 100), monitor).is_none());
    }
}
