use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// True while a fullscreen app, game or presentation is in the foreground on
/// the monitor the notch is on. With `island` (the island's window) the shell's
/// answer only counts when the foreground window is on the notch's monitor, so
/// a game on another monitor leaves the notch alone. With `None` it is the
/// shell's answer for the whole desktop. Returns false when the shell cannot
/// be queried.
pub fn is_fullscreen(island: Option<HWND>) -> bool {
    let Ok(state) = (unsafe { SHQueryUserNotificationState() }) else {
        return false;
    };
    let shell_fullscreen = matches!(
        state,
        QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
    );
    let Some(island) = island else {
        return shell_fullscreen;
    };
    // Only a fullscreen report needs the monitors; skip the reads otherwise.
    if !shell_fullscreen {
        return false;
    }
    let notch_monitor = monitor_of(island);
    let foreground = unsafe { GetForegroundWindow() };
    let foreground_monitor = if foreground.0.is_null() {
        None
    } else {
        monitor_of(foreground)
    };
    hides_notch(shell_fullscreen, foreground_monitor, notch_monitor)
}

/// The monitor a window is on (the nearest one), as an opaque id; `None` when
/// Windows gives no monitor.
fn monitor_of(hwnd: HWND) -> Option<isize> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    (!monitor.0.is_null()).then_some(monitor.0 as isize)
}

/// Whether a fullscreen report hides the notch: the shell says fullscreen and
/// either monitor is unknown (the shell's answer stands) or the foreground
/// window is on the notch's monitor. A single monitor always matches.
pub fn hides_notch(
    shell_fullscreen: bool,
    foreground_monitor: Option<isize>,
    notch_monitor: Option<isize>,
) -> bool {
    shell_fullscreen
        && match (foreground_monitor, notch_monitor) {
            (Some(fg), Some(notch)) => fg == notch,
            _ => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_off_is_never_hidden() {
        assert!(!hides_notch(false, Some(1), Some(1)));
        assert!(!hides_notch(false, Some(1), Some(2)));
        assert!(!hides_notch(false, None, None));
    }

    #[test]
    fn a_fullscreen_app_on_the_notchs_monitor_hides_it() {
        assert!(hides_notch(true, Some(7), Some(7)));
    }

    #[test]
    fn a_fullscreen_app_on_another_monitor_does_not_hide_it() {
        assert!(!hides_notch(true, Some(7), Some(8)));
    }

    #[test]
    fn an_unknown_foreground_keeps_the_shells_answer() {
        assert!(hides_notch(true, None, Some(7)));
    }

    #[test]
    fn an_unknown_notch_monitor_keeps_the_shells_answer() {
        assert!(hides_notch(true, Some(7), None));
        assert!(hides_notch(true, None, None));
    }
}
