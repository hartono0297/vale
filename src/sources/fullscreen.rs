use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GW_HWNDNEXT, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetDesktopWindow, GetForegroundWindow,
    GetShellWindow, GetSystemMetrics, GetTopWindow, GetWindow, GetWindowLongW, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed, SM_CMONITORS, WINDOW_EX_STYLE,
    WINDOW_STYLE, WS_CAPTION, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/// The most windows one z-order walk reads. A desktop has a few hundred
/// top-level windows; the walk stops at the first one that decides anyway.
const MAX_WALK: usize = 1024;

/// True while a fullscreen app, game or presentation covers the monitor the
/// notch is on. With `None` it is the shell's answer for the whole desktop
/// (`SHQueryUserNotificationState`, which only describes the foreground app);
/// false when the shell cannot be queried.
///
/// With `island` (the island's window), two steps:
/// 1. The shell says fullscreen and the foreground window is on the notch's
///    monitor (`hides_notch`): hidden.
/// 2. Otherwise, with two or more monitors, a known notch monitor and a
///    foreground window on another monitor, the shell cannot tell whether the
///    notch's own monitor shows a fullscreen video (the user clicked over to
///    the other screen). Then the top-level windows are walked top to bottom
///    (`top_candidate_covers`): the first real window that overlaps the notch's
///    monitor decides, hidden only if it covers the whole monitor. This runs at
///    most once per `TIMER_SYSTEM` tick, only in that case, and reads only as
///    many windows as it takes to reach the first one on that monitor.
pub fn is_fullscreen(island: Option<HWND>) -> bool {
    let shell_fullscreen = match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => matches!(
            state,
            QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
        ),
        Err(_) if island.is_none() => return false,
        // The walk does not need the shell: a failed query counts as "not fullscreen".
        Err(_) => false,
    };
    let Some(island) = island else {
        return shell_fullscreen;
    };
    let notch_monitor = monitor_of(island);
    let foreground = unsafe { GetForegroundWindow() };
    let foreground_monitor = if foreground.0.is_null() {
        None
    } else {
        monitor_of(foreground)
    };
    let monitor_count = u32::try_from(unsafe { GetSystemMetrics(SM_CMONITORS) }).unwrap_or(0);
    decide(
        shell_fullscreen,
        foreground_monitor,
        notch_monitor,
        monitor_count,
        || {
            let Some(monitor) = notch_monitor.and_then(monitor_rect) else {
                return false;
            };
            top_candidate_covers(z_order_facts(), monitor)
        },
    )
}

/// The whole decision for `is_fullscreen(Some(island))`, without Win32.
/// `walk` (the z-order scan of the notch's monitor) is called only when the
/// shell's report did not already hide the notch, there are two or more
/// monitors, both the foreground's and the notch's monitor are known and they
/// differ. Every other case keeps the shell's answer (`hides_notch`) and never
/// walks.
pub fn decide(
    shell_fullscreen: bool,
    foreground_monitor: Option<isize>,
    notch_monitor: Option<isize>,
    monitor_count: u32,
    walk: impl FnOnce() -> bool,
) -> bool {
    if hides_notch(shell_fullscreen, foreground_monitor, notch_monitor) {
        return true;
    }
    match (foreground_monitor, notch_monitor) {
        (Some(fg), Some(notch)) if monitor_count >= 2 && fg != notch => walk(),
        _ => false,
    }
}

/// A rectangle as edges, in virtual-screen pixels (`right` and `bottom` are
/// exclusive, like `RECT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect4 {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect4 {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    fn from_win(r: RECT) -> Self {
        Self::new(r.left, r.top, r.right, r.bottom)
    }
}

/// Whether two rectangles share a positive area. Touching edges, or an empty
/// rectangle, do not count.
pub fn intersects(a: Rect4, b: Rect4) -> bool {
    a.left < a.right
        && a.top < a.bottom
        && b.left < b.right
        && b.top < b.bottom
        && a.left < b.right
        && b.left < a.right
        && a.top < b.bottom
        && b.top < a.bottom
}

/// Whether a window covers a whole monitor: its rectangle contains the
/// monitor's full rectangle (not the work area), and it is not a maximised
/// window with a title bar. A maximised window with a caption never counts,
/// even with an auto-hiding taskbar. A borderless window sized to the work
/// area does not contain the monitor, so it does not count either.
pub fn covers_monitor(window: Rect4, monitor: Rect4, maximised_with_caption: bool) -> bool {
    !maximised_with_caption
        && window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

/// Plain facts about one top-level window, read from Win32 by `facts_of`.
#[derive(Clone, Copy, Debug)]
pub struct WindowFacts {
    /// The visible frame (`DWMWA_EXTENDED_FRAME_BOUNDS`).
    pub rect: Rect4,
    pub visible: bool,
    pub iconic: bool,
    pub cloaked: bool,
    /// Owned by this process: the island, the Susun overlay, the Identify plates.
    pub own_process: bool,
    /// The desktop, the wallpaper host or a taskbar.
    pub shell: bool,
    pub tool: bool,
    pub transparent: bool,
    pub maximised_with_caption: bool,
}

/// Whether a window can decide the question: a real, shown window. The
/// others (hidden, minimised, on another virtual desktop, the shell's own
/// windows, Vale's windows, tool windows, click-through overlays) are
/// skipped; they neither hide the notch nor stop the walk.
pub fn is_candidate(w: &WindowFacts) -> bool {
    w.visible && !w.iconic && !w.cloaked && !w.own_process && !w.shell && !w.tool && !w.transparent
}

/// Walks `windows` (top of the z-order first) to the first candidate that
/// shares an area with `monitor`, and says whether it covers the monitor.
/// False when no candidate overlaps the monitor. Lazy: it stops reading at the
/// first deciding window.
pub fn top_candidate_covers(
    windows: impl IntoIterator<Item = WindowFacts>,
    monitor: Rect4,
) -> bool {
    windows
        .into_iter()
        .filter(is_candidate)
        .find(|w| intersects(w.rect, monitor))
        .is_some_and(|w| covers_monitor(w.rect, monitor, w.maximised_with_caption))
}

/// Classes of the desktop and the taskbars.
fn is_shell_class_name(class: &str) -> bool {
    [
        "Progman",
        "WorkerW",
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
    ]
    .iter()
    .any(|c| c.eq_ignore_ascii_case(class))
}

/// The top-level windows, top of the z-order first, one `WindowFacts` each,
/// read when the iterator is advanced. Bounded by `MAX_WALK`.
fn z_order_facts() -> impl Iterator<Item = WindowFacts> {
    let (own_pid, shell, desktop) =
        unsafe { (GetCurrentProcessId(), GetShellWindow(), GetDesktopWindow()) };
    let first = unsafe { GetTopWindow(None) }.ok();
    std::iter::successors(first, |&hwnd| unsafe {
        GetWindow(hwnd, GW_HWNDNEXT).ok()
    })
    .take(MAX_WALK)
    .map(move |hwnd| unsafe { facts_of(hwnd, own_pid, shell, desktop) })
}

/// Reads the facts of one window. Anything already known to rule it out stops
/// the reads early (the rest stay at their harmless defaults).
unsafe fn facts_of(hwnd: HWND, own_pid: u32, shell: HWND, desktop: HWND) -> WindowFacts {
    unsafe {
        let mut facts = WindowFacts {
            rect: Rect4::new(0, 0, 0, 0),
            visible: IsWindowVisible(hwnd).as_bool(),
            iconic: IsIconic(hwnd).as_bool(),
            cloaked: false,
            own_process: false,
            shell: hwnd == shell || hwnd == desktop,
            tool: false,
            transparent: false,
            maximised_with_caption: false,
        };
        if !facts.visible || facts.iconic || facts.shell {
            return facts;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
        facts.own_process = pid == own_pid;
        let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
        facts.tool = ex.contains(WS_EX_TOOLWINDOW);
        facts.transparent = ex.contains(WS_EX_TRANSPARENT);
        if facts.own_process || facts.tool || facts.transparent {
            return facts;
        }
        let mut cloaked = 0u32;
        // A failed read counts as not cloaked.
        let read = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
        facts.cloaked = read.is_ok() && cloaked != 0;
        if facts.cloaked {
            return facts;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
        facts.shell = is_shell_class_name(&String::from_utf16_lossy(&class[..n]));
        let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
        facts.maximised_with_caption = IsZoomed(hwnd).as_bool() && style.contains(WS_CAPTION);
        facts.rect = frame_of(hwnd);
        facts
    }
}

/// The window's rectangle for the intersection and cover tests. It is the
/// visible frame (`DWMWA_EXTENDED_FRAME_BOUNDS`, as `arrange_win::visible_frame`)
/// and not `GetWindowRect`: a maximised window on the neighbouring monitor has
/// invisible resize borders that spill about 8 px into the notch's monitor in
/// `GetWindowRect`, which would make it the first window that overlaps that
/// monitor and wrongly show the notch over the video. `GetWindowRect` is only
/// the fallback when the DWM read fails.
unsafe fn frame_of(hwnd: HWND) -> Rect4 {
    unsafe {
        let mut frame = RECT::default();
        let read = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        );
        if read.is_err() {
            frame = RECT::default();
            let _ = GetWindowRect(hwnd, &mut frame);
        }
        Rect4::from_win(frame)
    }
}

/// A monitor's full rectangle (`rcMonitor`, not the work area).
fn monitor_rect(id: isize) -> Option<Rect4> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(HMONITOR(id as *mut core::ffi::c_void), &mut info) }
        .as_bool()
        .then(|| Rect4::from_win(info.rcMonitor))
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
    use std::cell::Cell;

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

    /// The notch's monitor: 1920x1080 at the origin. The other one sits to
    /// its right.
    const NOTCH: Rect4 = Rect4::new(0, 0, 1920, 1080);
    const OTHER: Rect4 = Rect4::new(1920, 0, 3840, 1080);

    fn window(rect: Rect4) -> WindowFacts {
        WindowFacts {
            rect,
            visible: true,
            iconic: false,
            cloaked: false,
            own_process: false,
            shell: false,
            tool: false,
            transparent: false,
            maximised_with_caption: false,
        }
    }

    fn fullscreen_video() -> WindowFacts {
        window(NOTCH)
    }

    fn normal_window() -> WindowFacts {
        window(Rect4::new(200, 100, 1200, 800))
    }

    #[test]
    fn touching_edges_do_not_intersect() {
        assert!(!intersects(OTHER, NOTCH));
        assert!(!intersects(NOTCH, OTHER));
        assert!(!intersects(Rect4::new(0, 1080, 100, 1200), NOTCH));
        assert!(intersects(Rect4::new(1919, 0, 2000, 10), NOTCH));
        assert!(!intersects(Rect4::new(10, 10, 10, 50), NOTCH));
    }

    #[test]
    fn the_top_candidate_covering_the_monitor_hides() {
        assert!(top_candidate_covers([fullscreen_video()], NOTCH));
        // Larger than the monitor (a game with a few spare pixels) still covers.
        assert!(top_candidate_covers(
            [window(Rect4::new(-8, -8, 1928, 1088))],
            NOTCH
        ));
    }

    #[test]
    fn a_normal_window_above_the_fullscreen_one_shows_it() {
        assert!(!top_candidate_covers(
            [normal_window(), fullscreen_video()],
            NOTCH
        ));
    }

    #[test]
    fn windows_on_other_monitors_are_ignored() {
        let touching = window(Rect4::new(1920, 0, 2600, 900));
        let beyond = window(Rect4::new(2000, 100, 3000, 900));
        let maximised_next_door = window(OTHER);
        assert!(top_candidate_covers(
            [touching, beyond, maximised_next_door, fullscreen_video()],
            NOTCH
        ));
    }

    #[test]
    fn every_non_candidate_above_the_fullscreen_window_is_skipped() {
        let cover = |tweak: fn(&mut WindowFacts)| {
            let mut w = normal_window();
            tweak(&mut w);
            w
        };
        let skipped = [
            cover(|w| w.own_process = true),
            cover(|w| w.tool = true),
            cover(|w| w.transparent = true),
            cover(|w| w.cloaked = true),
            cover(|w| w.iconic = true),
            cover(|w| w.visible = false),
            cover(|w| w.shell = true),
        ];
        for w in skipped {
            assert!(!is_candidate(&w));
            // It never hides on its own, even when it covers the monitor ...
            let mut covering = w;
            covering.rect = NOTCH;
            assert!(!top_candidate_covers([covering], NOTCH));
            // ... and it does not stop the walk to the fullscreen window.
            assert!(top_candidate_covers([w, fullscreen_video()], NOTCH));
            assert!(top_candidate_covers([covering, fullscreen_video()], NOTCH));
            // A non-candidate over a normal window leaves that window deciding.
            assert!(!top_candidate_covers([w, normal_window()], NOTCH));
        }
    }

    #[test]
    fn a_maximised_window_with_a_caption_does_not_count() {
        let mut maximised = window(Rect4::new(-8, -8, 1928, 1088));
        maximised.maximised_with_caption = true;
        assert!(!top_candidate_covers([maximised], NOTCH));
        assert!(!covers_monitor(maximised.rect, NOTCH, true));
        // It is a candidate, so it decides over the fullscreen window below.
        assert!(!top_candidate_covers([maximised, fullscreen_video()], NOTCH));
    }

    #[test]
    fn a_borderless_window_the_size_of_the_work_area_does_not_count() {
        // The taskbar (48 px) is visible along the bottom.
        let work_area = window(Rect4::new(0, 0, 1920, 1032));
        assert!(!top_candidate_covers([work_area], NOTCH));
    }

    #[test]
    fn no_intersecting_candidate_is_false() {
        assert!(!top_candidate_covers(Vec::new(), NOTCH));
        assert!(!top_candidate_covers([window(OTHER)], NOTCH));
        let mut hidden = fullscreen_video();
        hidden.visible = false;
        assert!(!top_candidate_covers([hidden], NOTCH));
    }

    #[test]
    fn the_walk_stops_at_the_first_deciding_window() {
        let read = Cell::new(0);
        let windows = (0..10).map(|i| {
            read.set(read.get() + 1);
            if i == 2 { fullscreen_video() } else { window(OTHER) }
        });
        assert!(top_candidate_covers(windows, NOTCH));
        assert_eq!(read.get(), 3);
    }

    #[test]
    fn shell_classes_are_recognised() {
        for class in ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
            assert!(is_shell_class_name(class));
        }
        assert!(is_shell_class_name("progman"));
        assert!(!is_shell_class_name("Chrome_WidgetWin_1"));
    }

    fn no_walk() -> bool {
        panic!("the walk must not run");
    }

    #[test]
    fn one_monitor_never_walks() {
        assert!(decide(true, Some(1), Some(1), 1, no_walk));
        assert!(!decide(false, Some(1), Some(1), 1, no_walk));
        assert!(!decide(false, Some(1), Some(2), 1, no_walk));
        assert!(!decide(true, Some(1), Some(2), 1, no_walk));
    }

    #[test]
    fn the_foreground_on_the_notch_monitor_never_walks() {
        assert!(decide(true, Some(7), Some(7), 2, no_walk));
        assert!(!decide(false, Some(7), Some(7), 2, no_walk));
    }

    #[test]
    fn the_foreground_elsewhere_walks_and_returns_the_walks_answer() {
        let walked = Cell::new(false);
        assert!(decide(false, Some(8), Some(7), 2, || {
            walked.set(true);
            true
        }));
        assert!(walked.get());
        assert!(!decide(false, Some(8), Some(7), 2, || false));
        assert!(decide(false, Some(8), Some(7), 3, || true));
    }

    #[test]
    fn an_unknown_foreground_or_notch_monitor_never_walks() {
        assert!(decide(true, None, Some(7), 2, no_walk));
        assert!(!decide(false, None, Some(7), 2, no_walk));
        assert!(decide(true, Some(7), None, 2, no_walk));
        assert!(!decide(false, Some(7), None, 2, no_walk));
        assert!(decide(true, None, None, 2, no_walk));
        assert!(!decide(false, None, None, 2, no_walk));
    }

    #[test]
    fn shell_fullscreen_elsewhere_still_defers_to_the_walk() {
        // A game on monitor 2 while the notch's monitor shows nothing fullscreen.
        assert!(!decide(true, Some(8), Some(7), 2, || false));
        // A game on monitor 2 and a fullscreen video on the notch's monitor.
        assert!(decide(true, Some(8), Some(7), 2, || true));
    }
}
