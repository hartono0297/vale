//! "Tandai" (Identify): a dark rounded plate with the display's number in the
//! middle of every screen, for about two seconds.
//!
//! Each plate is its own small layered popup (never the whole screen, so the
//! shell does not read it as a fullscreen app), click-through, never activated
//! and drawn with `UpdateLayeredWindow` like Susun's overlays (the `Canvas` of
//! `arrange_overlay`). The hold and the fade run on timers of the first
//! plate's own window; the fade timer exists only while a fade runs. Nothing
//! here ever calls into `App`: the window procedure only touches this
//! module's state.

use std::cell::RefCell;
use std::time::Instant;

use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, IDC_ARROW, KillTimer, LoadCursorW,
    RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SetTimer, ShowWindow, WM_ERASEBKGND, WM_TIMER,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Error, w};

use crate::arrange::Rect;
use crate::arrange_overlay::Canvas;
use crate::notch_monitor::Display;

/// Named in `capture::win::KEPT_OUT`, so a screenshot never holds a plate.
pub const CLASS_NAME: &str = "ValeIdentify";

/// How long the plates stay at full strength.
pub const HOLD_MS: u32 = 2000;
/// With fades on: in over this long, out over that.
pub const FADE_IN_MS: u32 = 120;
pub const FADE_OUT_MS: u32 = 200;
/// A fade frame (only while one runs).
const FADE_FRAME_MS: u32 = 16;
const TIMER_HOLD: usize = 1;
const TIMER_FADE: usize = 2;

/// The plate: 160 dip square, 24 dip corners, #1B1D1F at 90 %, the number 96
/// dip tall in white.
const PLATE_DIP: f32 = 160.0;
const PLATE_RADIUS_DIP: f32 = 24.0;
const PLATE_RGB: u32 = 0x1B1D1F;
const PLATE_ALPHA: f32 = 0.90;
const DIGIT_DIP: f32 = 96.0;
const DIGIT_RGB: u32 = 0xFFFFFF;

/// The plate's rectangle on a monitor `monitor` (physical px) of `dpi`:
/// 160 dip square, centred.
pub fn plate_rect(monitor: Rect, dpi: u32) -> Rect {
    let side = (PLATE_DIP * dpi.max(96) as f32 / 96.0).round() as i32;
    Rect::new(
        monitor.x + (monitor.w - side) / 2,
        monitor.y + (monitor.h - side) / 2,
        side,
        side,
    )
}

/// The opacity (0..=1) `elapsed_ms` into a fade in that started at `from`.
pub fn fade_in_alpha(from: f32, elapsed_ms: u32) -> f32 {
    let t = (elapsed_ms as f32 / FADE_IN_MS as f32).clamp(0.0, 1.0);
    from + (1.0 - from) * t
}

/// The opacity `elapsed_ms` into a fade out that started at `from`.
pub fn fade_out_alpha(from: f32, elapsed_ms: u32) -> f32 {
    let t = (elapsed_ms as f32 / FADE_OUT_MS as f32).clamp(0.0, 1.0);
    from * (1.0 - t)
}

/// A plate's whole-window opacity as `UpdateLayeredWindow` takes it.
fn opacity(alpha: f32) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

struct Plate {
    hwnd: HWND,
    canvas: Canvas,
    origin: (i32, i32),
    side: i32,
}

#[derive(Clone, Copy)]
enum Phase {
    Hidden,
    In { since: Instant, from: f32 },
    Hold,
    Out { since: Instant, from: f32 },
}

struct Overlay {
    plates: Vec<Plate>,
    phase: Phase,
    /// How strong the plates show now, 0..=1.
    alpha: f32,
    fade: bool,
}

thread_local! {
    static OVERLAY: RefCell<Overlay> = const {
        RefCell::new(Overlay {
            plates: Vec::new(),
            phase: Phase::Hidden,
            alpha: 0.0,
            fade: false,
        })
    };
}

fn class() -> windows::core::PCWSTR {
    w!("ValeIdentify")
}

fn register() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(proc),
        hInstance: module.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: class(),
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            return Error::from_thread().code() == ERROR_CLASS_ALREADY_EXISTS.to_hresult();
        }
    }
    true
}

fn create_window(area: Rect) -> Option<HWND> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class(),
            w!("Vale identify"),
            WS_POPUP,
            area.x,
            area.y,
            area.w,
            area.h,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .ok()
}

/// Numbers every display in `displays` for about two seconds. With `fade` the
/// plates fade in over 120 ms and out over 200 ms; without it they just show
/// and hide. Calling it again restarts the hold and refreshes the numbers and
/// the places. An empty list shows nothing.
pub fn show(displays: &[Display], fade: bool) {
    if !register() {
        return;
    }
    OVERLAY.with(|o| {
        // Never re-entered by our own window procedure while we draw, but be
        // safe: a nested call simply does nothing.
        if let Ok(mut o) = o.try_borrow_mut() {
            o.show(displays, fade);
        }
    });
}

impl Overlay {
    fn show(&mut self, displays: &[Display], fade: bool) {
        self.fade = fade;
        // Plates that no longer have a display go; the rest are reused.
        while self.plates.len() > displays.len() {
            if let Some(p) = self.plates.pop() {
                unsafe {
                    let _ = DestroyWindow(p.hwnd);
                }
            }
        }
        let start = if fade { self.alpha } else { 1.0 };
        for (i, d) in displays.iter().enumerate() {
            let area = plate_rect(d.monitor.rect, d.monitor.dpi);
            let reusable = self.plates.get(i).is_some_and(|p| p.side == area.w);
            if !reusable {
                if i < self.plates.len() {
                    let old = self.plates.remove(i);
                    unsafe {
                        let _ = DestroyWindow(old.hwnd);
                    }
                }
                let Some(hwnd) = create_window(area) else {
                    continue;
                };
                let Ok(canvas) = Canvas::new((area.w, area.h)) else {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                    continue;
                };
                let plate = Plate {
                    hwnd,
                    canvas,
                    origin: (area.x, area.y),
                    side: area.w,
                };
                let at = i.min(self.plates.len());
                self.plates.insert(at, plate);
            }
            let Some(plate) = self.plates.get_mut(i) else {
                continue;
            };
            plate.origin = (area.x, area.y);
            let scale = area.w as f32 / PLATE_DIP;
            let side = area.w as f32;
            let number = d.number.to_string();
            let drawn =
                plate
                    .canvas
                    .paint_with(plate.hwnd, plate.origin, 0.0, opacity(start), |c| {
                        c.fill(
                            Rect::new(0, 0, area.w, area.h),
                            PLATE_RADIUS_DIP * scale,
                            PLATE_RGB,
                            PLATE_ALPHA,
                        );
                        c.text(
                            &number,
                            (0.0, 0.0, side, side),
                            DIGIT_DIP * scale,
                            DIGIT_RGB,
                            1.0,
                        );
                    });
            if drawn.is_ok() {
                unsafe {
                    let _ = ShowWindow(plate.hwnd, SW_SHOWNOACTIVATE);
                }
            }
        }
        let Some(first) = self.plates.first().map(|p| p.hwnd) else {
            self.phase = Phase::Hidden;
            self.alpha = 0.0;
            return;
        };
        self.alpha = start;
        if fade {
            self.phase = Phase::In {
                since: Instant::now(),
                from: start,
            };
            unsafe {
                SetTimer(Some(first), TIMER_FADE, FADE_FRAME_MS, None);
            }
        } else {
            self.phase = Phase::Hold;
            unsafe {
                let _ = KillTimer(Some(first), TIMER_FADE);
            }
        }
        // The same id on the same window: this restarts the hold.
        unsafe {
            SetTimer(Some(first), TIMER_HOLD, HOLD_MS, None);
        }
    }

    fn present_all(&self) {
        let o = opacity(self.alpha);
        for p in &self.plates {
            let _ = p.canvas.present(p.hwnd, p.origin, o);
        }
    }

    fn kill_timers(&self) {
        if let Some(first) = self.plates.first() {
            unsafe {
                let _ = KillTimer(Some(first.hwnd), TIMER_HOLD);
                let _ = KillTimer(Some(first.hwnd), TIMER_FADE);
            }
        }
    }

    fn hide(&mut self) {
        self.kill_timers();
        for p in &self.plates {
            unsafe {
                let _ = ShowWindow(p.hwnd, SW_HIDE);
            }
        }
        self.phase = Phase::Hidden;
        self.alpha = 0.0;
    }

    fn on_timer(&mut self, id: usize) {
        let Some(first) = self.plates.first().map(|p| p.hwnd) else {
            return;
        };
        match id {
            TIMER_HOLD => {
                unsafe {
                    let _ = KillTimer(Some(first), TIMER_HOLD);
                }
                if self.fade {
                    self.phase = Phase::Out {
                        since: Instant::now(),
                        from: self.alpha,
                    };
                    unsafe {
                        SetTimer(Some(first), TIMER_FADE, FADE_FRAME_MS, None);
                    }
                } else {
                    self.hide();
                }
            }
            TIMER_FADE => match self.phase {
                Phase::In { since, from } => {
                    let ms = since.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
                    self.alpha = fade_in_alpha(from, ms);
                    self.present_all();
                    if self.alpha >= 1.0 {
                        self.phase = Phase::Hold;
                        unsafe {
                            let _ = KillTimer(Some(first), TIMER_FADE);
                        }
                    }
                }
                Phase::Out { since, from } => {
                    let ms = since.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
                    self.alpha = fade_out_alpha(from, ms);
                    if self.alpha <= 0.0 {
                        self.hide();
                    } else {
                        self.present_all();
                    }
                }
                Phase::Hidden | Phase::Hold => unsafe {
                    let _ = KillTimer(Some(first), TIMER_FADE);
                },
            },
            _ => {}
        }
    }
}

/// Never calls into `App`: it only reads and writes this module's state.
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            OVERLAY.with(|o| {
                if let Ok(mut o) = o.try_borrow_mut() {
                    o.on_timer(wparam.0);
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_class_name_matches_what_screenshots_keep_out() {
        assert_eq!(CLASS_NAME, "ValeIdentify");
    }

    #[test]
    fn the_plate_is_160_dip_and_centred_on_its_monitor() {
        let r = plate_rect(Rect::new(0, 0, 1920, 1080), 96);
        assert_eq!((r.w, r.h), (160, 160));
        assert_eq!((r.x, r.y), ((1920 - 160) / 2, (1080 - 160) / 2));
        // A monitor to the left of the origin, and one below it.
        let r = plate_rect(Rect::new(-2560, 200, 2560, 1440), 96);
        assert_eq!(
            (r.x, r.y),
            (-2560 + (2560 - 160) / 2, 200 + (1440 - 160) / 2)
        );
    }

    #[test]
    fn the_plate_scales_with_the_monitors_dpi() {
        let r = plate_rect(Rect::new(1920, 0, 3840, 2160), 192);
        assert_eq!((r.w, r.h), (320, 320));
        assert_eq!(r.x, 1920 + (3840 - 320) / 2);
        assert_eq!(r.y, (2160 - 320) / 2);
        let r = plate_rect(Rect::new(0, 0, 2400, 1350), 120);
        assert_eq!(r.w, 200);
        // The plate's centre is the monitor's (to a pixel).
        assert!((r.x + r.w / 2 - 1200).abs() <= 1);
        assert!((r.y + r.h / 2 - 675).abs() <= 1);
    }

    #[test]
    fn a_fade_in_runs_from_where_it_started_to_full_in_120_ms() {
        assert_eq!(fade_in_alpha(0.0, 0), 0.0);
        assert!((fade_in_alpha(0.0, 60) - 0.5).abs() < 1e-6);
        assert_eq!(fade_in_alpha(0.0, FADE_IN_MS), 1.0);
        assert_eq!(fade_in_alpha(0.0, 5000), 1.0);
        // Restarting mid-fade carries on from the strength showing.
        assert!((fade_in_alpha(0.5, 0) - 0.5).abs() < 1e-6);
        assert!((fade_in_alpha(0.5, 60) - 0.75).abs() < 1e-6);
        let mut last = 0.0;
        for ms in (0..=FADE_IN_MS).step_by(8) {
            let a = fade_in_alpha(0.0, ms);
            assert!(a >= last);
            last = a;
        }
    }

    #[test]
    fn a_fade_out_runs_to_nothing_in_200_ms() {
        assert_eq!(fade_out_alpha(1.0, 0), 1.0);
        assert!((fade_out_alpha(1.0, 100) - 0.5).abs() < 1e-6);
        assert_eq!(fade_out_alpha(1.0, FADE_OUT_MS), 0.0);
        assert_eq!(fade_out_alpha(1.0, 9999), 0.0);
        // Starting from less than full leaves from there.
        assert!((fade_out_alpha(0.4, 100) - 0.2).abs() < 1e-6);
        assert_eq!(opacity(1.0), 255);
        assert_eq!(opacity(0.0), 0);
        assert_eq!(opacity(2.0), 255);
        assert_eq!(opacity(-1.0), 0);
    }
}
