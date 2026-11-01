use std::collections::HashSet;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use windows::Foundation::{Size, TypedEventHandler};
use windows::Storage::Streams::DataReader;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeMedianCut,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoCreateInstance,
    CoInitializeEx,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{Interface, Result};

use crate::clipboard::image::Thumb;
use crate::i18n::{Key, tr};
use crate::model::{Event, Notice, NoticeKind, Toast, ToastOrigin};
use crate::notif::logo_key;

/// A logo is read at this size and kept at most this many pixels on a side.
pub const LOGO_SIDE: u32 = 48;
/// A logo stream bigger than this is not read.
const LOGO_MAX_BYTES: u64 = 1024 * 1024;

const PROBE_SECONDS: u32 = 60;
/// Re-read interval when change events are unavailable.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// Re-read interval when change events are delivered, so a missed event
/// delays a toast by at most this long.
const SAFETY_NET_INTERVAL: Duration = Duration::from_secs(60);

/// Reports which notification ids are new since the previous poll. The first
/// update only records ids, so notifications already in the Action Center
/// when the app starts are not shown.
#[derive(Default)]
pub struct NotificationTracker {
    seen: HashSet<u32>,
    primed: bool,
}

impl NotificationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the ids in `current` that were absent from the previous update,
    /// in the order given. Ids that disappeared are forgotten.
    pub fn update(&mut self, current: &[u32]) -> Vec<u32> {
        let fresh = if self.primed {
            current
                .iter()
                .copied()
                .filter(|id| !self.seen.contains(id))
                .collect()
        } else {
            Vec::new()
        };
        self.seen = current.iter().copied().collect();
        self.primed = true;
        fresh
    }
}

/// True when any muted entry occurs in `source`, ignoring case.
pub fn is_muted(source: &str, muted: &[String]) -> bool {
    let source = source.to_lowercase();
    muted.iter().any(|m| source.contains(&m.to_lowercase()))
}

/// A `NotificationChanged` handler on the listener; removed when dropped.
struct Subscription {
    listener: UserNotificationListener,
    token: i64,
}

impl Subscription {
    /// Registers a handler that sends one signal per change. Fails for apps
    /// without package identity ("Element not found").
    fn new(listener: &UserNotificationListener, signals: Sender<()>) -> Result<Self> {
        let token = listener.NotificationChanged(&TypedEventHandler::new(move |_, _| {
            let _ = signals.send(());
            Ok(())
        }))?;
        Ok(Self {
            listener: listener.clone(),
            token,
        })
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.listener.RemoveNotificationChanged(self.token);
    }
}

/// How the poller learns that the Action Center may have changed.
enum Trigger {
    /// Change events arrive; re-read on each burst, and every
    /// `SAFETY_NET_INTERVAL` in case one was missed.
    Events {
        _subscription: Subscription,
        signals: Receiver<()>,
        /// At least one event has been delivered since subscribing.
        delivered: bool,
    },
    /// No usable events; re-read every `POLL_INTERVAL`.
    Poll,
}

impl Trigger {
    /// Event mode when the listener accepts a change handler, else polling.
    fn new(listener: &UserNotificationListener) -> Self {
        let (tx, signals) = mpsc::channel();
        match Subscription::new(listener, tx) {
            Ok(subscription) => Trigger::Events {
                _subscription: subscription,
                signals,
                delivered: false,
            },
            Err(_) => Trigger::Poll,
        }
    }

    /// Blocks until the next re-read is due. Returns true when woken by a
    /// change event rather than by the timer.
    fn wait(&mut self) -> bool {
        match self {
            Trigger::Events {
                signals, delivered, ..
            } => {
                let signalled = wait_for_signal(signals, SAFETY_NET_INTERVAL);
                *delivered |= signalled;
                signalled
            }
            Trigger::Poll => {
                std::thread::sleep(POLL_INTERVAL);
                false
            }
        }
    }

    /// Drops to polling when the subscription has never delivered an event
    /// yet a timed re-read found new toasts: the event is not firing.
    fn after_fetch(&mut self, signalled: bool, found_new: bool) {
        if let Trigger::Events { delivered, .. } = self
            && events_not_firing(*delivered, signalled, found_new)
        {
            *self = Trigger::Poll;
        }
    }
}

/// True when a safety-net re-read (not woken by an event) found new toasts
/// although no change event has ever arrived.
fn events_not_firing(delivered: bool, signalled: bool, found_new: bool) -> bool {
    !delivered && !signalled && found_new
}

/// Waits up to `timeout` for a signal, then drains any queued behind it so a
/// burst costs one re-read. True when at least one signal arrived.
fn wait_for_signal(signals: &Receiver<()>, timeout: Duration) -> bool {
    match signals.recv_timeout(timeout) {
        Ok(()) => {
            signals.try_iter().for_each(drop);
            true
        }
        Err(RecvTimeoutError::Timeout) => false,
        // The handler holding the sender is gone; never spin.
        Err(RecvTimeoutError::Disconnected) => {
            std::thread::sleep(timeout);
            false
        }
    }
}

/// Watches the Action Center on a background thread and sends an
/// `Event::Toast` for every new toast, whatever app it is from: `App` decides
/// (muted apps, the history). The first time a logo key is seen this run its
/// app logo rides along. Re-reads on `NotificationChanged` events when the
/// listener supports them, else polls once a second. Exits quietly when
/// listener access has not been granted.
pub fn spawn_poller(tx: Sender<Event>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let Ok(listener) = UserNotificationListener::Current() else {
            return;
        };
        if listener.GetAccessStatus().ok() != Some(UserNotificationListenerAccessStatus::Allowed) {
            return;
        }

        let mut trigger = Trigger::new(&listener);
        let mut tracker = NotificationTracker::new();
        let mut signalled = false;
        // Logo keys looked at this run; a key that failed is not retried.
        let mut tried: HashSet<String> = HashSet::new();
        loop {
            if let Ok(fresh) = new_notifications(&listener, &mut tracker) {
                trigger.after_fetch(signalled, !fresh.is_empty());
                for toast in &fresh {
                    let Some(notice) = to_notice(toast) else {
                        continue;
                    };
                    let app_id = match &notice.kind {
                        NoticeKind::App { app_id, .. } => app_id.as_deref(),
                        _ => None,
                    };
                    let key = logo_key(app_id, &notice.source);
                    let logo = if tried.insert(key.clone()) {
                        read_logo(toast).map(|thumb| (key, thumb))
                    } else {
                        None
                    };
                    let at = chrono::Local::now().timestamp();
                    if tx.send(Event::Toast(Toast { notice, at, logo })).is_err() {
                        return;
                    }
                    wake();
                }
            }
            signalled = trigger.wait();
        }
    });
}

/// True when this app may read the Action Center.
pub fn access_allowed() -> bool {
    UserNotificationListener::Current()
        .and_then(|listener| listener.GetAccessStatus())
        .is_ok_and(|status| status == UserNotificationListenerAccessStatus::Allowed)
}

/// Asks the user for listener access without blocking; `done` runs (on any
/// thread) once they have answered. Call from the UI thread.
pub fn ask_access(done: impl FnOnce() + Send + 'static) -> Result<()> {
    UserNotificationListener::Current()?
        .RequestAccessAsync()?
        .when(move |_| done())
}

/// Brings the app with this AppUserModelID to the front, launching it if needed.
pub fn open_app(app_id: &str) {
    let _ = Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{app_id}"))
        .spawn();
}

/// Where the user grants (or takes back) notification access.
pub const PRIVACY_PAGE: &str = "ms-settings:privacy-notifications";

/// Opens Windows' notification access page. Never asks for access itself.
pub fn open_privacy_settings() {
    let _ = Command::new("explorer.exe").arg(PRIVACY_PAGE).spawn();
}

/// Diagnostic mode for `--probe-notifications`: requests listener access, then
/// prints every toast that arrives during the next 60 seconds.
pub fn probe() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let listener = match request_access() {
        Ok(Some(listener)) => listener,
        Ok(None) => {
            println!("Izin notifikasi belum diberikan. Buka: ms-settings:privacy-notifications");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("vale: notification listener unavailable: {e}");
            std::process::exit(1);
        }
    };

    match Subscription::new(&listener, mpsc::channel().0) {
        Ok(_) => println!("Change events: subscribed; the island re-reads on each change."),
        Err(e) => println!("Change events unavailable ({e}); the island polls every second."),
    }
    println!("Listening for notifications for {PROBE_SECONDS} s...");
    let mut tracker = NotificationTracker::new();
    for _ in 0..PROBE_SECONDS {
        match new_notifications(&listener, &mut tracker) {
            Ok(fresh) => {
                for toast in &fresh {
                    print_probe(toast);
                }
            }
            Err(e) => eprintln!("vale: reading notifications failed: {e}"),
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// One fresh toast for `--probe-notifications`: the notice line the island
/// would build, then the raw facts it was built from (display name, AUMID,
/// every raw text element) and the site `split_site` picks, so the owner can
/// see whether a browser's attribution line arrives.
fn print_probe(toast: &UserNotification) {
    if let Some(notice) = to_notice(toast) {
        println!("{} | {} | {}", notice.source, notice.title, notice.body);
    }
    let name = app_name(toast).ok();
    let app_id = app_id_of(toast);
    println!("  app: {}", name.as_deref().unwrap_or("-"));
    println!("  aumid: {}", app_id.as_deref().unwrap_or("-"));
    let raw = text_elements(toast);
    match &raw {
        Ok(items) => {
            for (i, text) in items.iter().enumerate() {
                println!("  [{i}] {text:?}");
            }
        }
        Err(e) => println!("  text elements unreadable: {e}"),
    }
    let source = source_name(name, app_id.clone());
    let (_, site) = split_site(app_id.as_deref(), &source, raw.unwrap_or_default());
    println!("  site: {}", site.as_deref().unwrap_or("-"));
}

fn request_access() -> Result<Option<UserNotificationListener>> {
    let listener = UserNotificationListener::Current()?;
    let status = listener.RequestAccessAsync()?.join()?;
    Ok((status == UserNotificationListenerAccessStatus::Allowed).then_some(listener))
}

/// Toasts that appeared since the previous call, in Action Center order.
/// Toasts whose id cannot be read are ignored.
fn new_notifications(
    listener: &UserNotificationListener,
    tracker: &mut NotificationTracker,
) -> Result<Vec<UserNotification>> {
    let current: Vec<(u32, UserNotification)> = listener
        .GetNotificationsAsync(NotificationKinds::Toast)?
        .join()?
        .into_iter()
        .filter_map(|n| n.Id().ok().map(|id| (id, n)))
        .collect();
    let ids: Vec<u32> = current.iter().map(|(id, _)| *id).collect();
    let fresh = tracker.update(&ids);
    Ok(current
        .into_iter()
        .filter(|(id, _)| fresh.contains(id))
        .map(|(_, n)| n)
        .collect())
}

/// The size a logo of `w` x `h` is kept at: fitted inside `LOGO_SIDE`
/// square keeping its proportions, never enlarged, at least 1 x 1.
pub fn fit_logo(w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    if w <= LOGO_SIDE && h <= LOGO_SIDE {
        return (w, h);
    }
    let k = f64::from(LOGO_SIDE) / f64::from(w.max(h));
    let scale = |n: u32| ((f64::from(n) * k).round() as u32).clamp(1, LOGO_SIDE);
    (scale(w), scale(h))
}

/// The app's logo as a premultiplied BGRA picture of at most
/// `LOGO_SIDE` square. Runs on the poller thread (never the UI thread);
/// `None` on any failure (the page draws a letter tile instead).
fn read_logo(notification: &UserNotification) -> Option<Thumb> {
    let reference = notification
        .AppInfo()
        .ok()?
        .DisplayInfo()
        .ok()?
        .GetLogo(Size {
            Width: LOGO_SIDE as f32,
            Height: LOGO_SIDE as f32,
        })
        .ok()?;
    let stream = reference.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > LOGO_MAX_BYTES {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(size as u32).ok()?.join().ok()?;
    let mut bytes = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    decode_logo(&bytes)
}

/// Decodes encoded image bytes (PNG, ICO, ...) with WIC, scaled to fit.
fn decode_logo(bytes: &[u8]) -> Option<Thumb> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = SHCreateMemStream(Some(bytes))?;
        let decoder = wic
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let (tw, th) = fit_logo(w, h);
        let source: IWICBitmapSource = if (tw, th) != (w, h) {
            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&frame, tw, th, WICBitmapInterpolationModeFant)
                .ok()?;
            scaler.cast().ok()?
        } else {
            frame.cast().ok()?
        };
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
            .ok()?;
        let mut bgra = vec![0u8; (tw * th * 4) as usize];
        converter
            .CopyPixels(std::ptr::null(), tw * 4, &mut bgra)
            .ok()?;
        Some(Thumb { w: tw, h: th, bgra })
    }
}

/// Maps a toast to a notice; `None` when it has no readable id.
fn to_notice(notification: &UserNotification) -> Option<Notice> {
    let key = notification.Id().ok()?.to_string();
    let app_id = app_id_of(notification);
    let source = source_name(app_name(notification).ok(), app_id.clone());
    let (texts, site) = split_site(
        app_id.as_deref(),
        &source,
        text_elements(notification).unwrap_or_default(),
    );
    let (title, body) = notice_text(&source, texts);
    Some(Notice {
        key,
        source,
        title,
        body,
        kind: NoticeKind::App {
            app_id,
            toast: Some(ToastOrigin { site }),
        },
    })
}

fn app_id_of(notification: &UserNotification) -> Option<String> {
    notification
        .AppInfo()
        .and_then(|info| info.AppUserModelId())
        .ok()
        .map(|id| id.to_string())
}

/// Browsers whose toasts may carry the web site as their last text line.
const BROWSER_WORDS: [&str; 7] = [
    "chrome", "msedge", "edge", "firefox", "brave", "opera", "vivaldi",
];

/// True when the AUMID or the display name has a browser's name as a whole
/// word ("Knowledge" is not Edge).
fn is_browser(app_id: Option<&str>, source: &str) -> bool {
    let has_browser_word = |text: &str| {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| BROWSER_WORDS.contains(&word))
    };
    app_id.is_some_and(has_browser_word) || has_browser_word(source)
}

/// `text` as a bare host ("web.telegram.org", "localhost.test:8080"),
/// optionally behind "via ": no spaces, at least one dot, letters, digits,
/// '.' and '-' only, no empty label, no leading or trailing '.' or '-', an
/// optional ":port" of 1-5 digits. Returned as given, without "via ".
fn bare_host(text: &str) -> Option<String> {
    let text = text.trim();
    let rest = match text.get(..4) {
        Some(head) if head.eq_ignore_ascii_case("via ") => text[4..].trim_start(),
        _ => text,
    };
    let (host, port) = match rest.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (rest, None),
    };
    if let Some(port) = port
        && !((1..=5).contains(&port.len()) && port.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let ok_char = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    let edge = |c: char| c == '.' || c == '-';
    let valid = host.contains('.')
        && host.chars().all(ok_char)
        && !host.starts_with(edge)
        && !host.ends_with(edge)
        && host.split('.').all(|label| !label.is_empty());
    valid.then(|| rest.to_string())
}

/// A browser toast's last line is the site when it is a bare host: it moves
/// out of the texts. The title (index 0) is never taken. Anything else comes
/// back unchanged with no site.
fn split_site(
    app_id: Option<&str>,
    source: &str,
    mut texts: Vec<String>,
) -> (Vec<String>, Option<String>) {
    if texts.len() >= 2
        && is_browser(app_id, source)
        && let Some(site) = texts.last().and_then(|last| bare_host(last))
    {
        texts.pop();
        return (texts, Some(site));
    }
    (texts, None)
}

/// The app's display name, else its AppUserModelID, else a generic label, so
/// a toast is never dropped for an unreadable name.
fn source_name(display_name: Option<String>, app_id: Option<String>) -> String {
    [display_name, app_id]
        .into_iter()
        .flatten()
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| tr(Key::SourceNotification).to_string())
}

/// Title and body for a toast's text elements, each on a single line: the
/// island draws them in one-line boxes, where a hard line break would push
/// the rest out of view. The first element is the title (the app name when
/// there is none); the rest join with " · ".
fn notice_text(source: &str, texts: Vec<String>) -> (String, String) {
    let mut texts = texts.iter().map(|text| one_line(text));
    let title = texts.next().unwrap_or_else(|| source.to_string());
    let body = texts
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    (title, body)
}

/// Replaces each run of `\r` / `\n` with a single space.
fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_break = false;
    for c in text.chars() {
        if c == '\r' || c == '\n' {
            if !in_break {
                out.push(' ');
            }
            in_break = true;
        } else {
            out.push(c);
            in_break = false;
        }
    }
    out
}

fn app_name(notification: &UserNotification) -> Result<String> {
    Ok(notification
        .AppInfo()?
        .DisplayInfo()?
        .DisplayName()?
        .to_string())
}

fn text_elements(notification: &UserNotification) -> Result<Vec<String>> {
    let binding = notification
        .Notification()?
        .Visual()?
        .GetBinding(&KnownNotificationBindings::ToastGeneric()?)?;
    binding
        .GetTextElements()?
        .into_iter()
        .map(|text| Ok(text.Text()?.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_update_primes_without_showing() {
        let mut t = NotificationTracker::new();
        assert!(t.update(&[1, 2, 3]).is_empty());
        assert_eq!(t.update(&[1, 2, 3, 4]), vec![4]);
    }

    #[test]
    fn returns_new_ids_in_given_order() {
        let mut t = NotificationTracker::new();
        t.update(&[1]);
        assert_eq!(t.update(&[1, 7, 5]), vec![7, 5]);
    }

    #[test]
    fn forgets_ids_no_longer_present() {
        let mut t = NotificationTracker::new();
        t.update(&[1, 2]);
        assert!(t.update(&[2]).is_empty());
        assert_eq!(t.seen.len(), 1);
    }

    #[test]
    fn signal_burst_is_drained_in_one_wait() {
        let (tx, rx) = mpsc::channel();
        for _ in 0..5 {
            tx.send(()).unwrap();
        }
        assert!(wait_for_signal(&rx, Duration::from_millis(10)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn wait_times_out_without_signal() {
        let (_tx, rx) = mpsc::channel::<()>();
        assert!(!wait_for_signal(&rx, Duration::from_millis(10)));
    }

    #[test]
    fn disconnected_signals_still_wait_the_full_timeout() {
        let (tx, rx) = mpsc::channel::<()>();
        drop(tx);
        let start = std::time::Instant::now();
        assert!(!wait_for_signal(&rx, Duration::from_millis(30)));
        assert!(start.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn falls_back_only_when_timer_finds_toasts_events_never_reported() {
        assert!(events_not_firing(false, false, true));
        assert!(!events_not_firing(true, false, true));
        assert!(!events_not_firing(false, true, true));
        assert!(!events_not_firing(false, false, false));
    }

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn multi_line_elements_become_one_line() {
        let (title, body) =
            notice_text("Chrome", texts(&["Ibu", "baris 1\nbaris 2", "via Chrome"]));
        assert_eq!(title, "Ibu");
        assert_eq!(body, "baris 1 baris 2 · via Chrome");
    }

    #[test]
    fn newline_runs_collapse_to_one_space_in_title_too() {
        let (title, body) = notice_text("App", texts(&["Judul\r\n\r\nlanjut", "a\n\nb\rc"]));
        assert_eq!(title, "Judul lanjut");
        assert_eq!(body, "a b c");
    }

    #[test]
    fn empty_body_elements_are_skipped() {
        let (_, body) = notice_text("App", texts(&["T", "", "x", "\n"]));
        assert_eq!(body, "x");
    }

    #[test]
    fn toast_without_text_is_titled_with_source() {
        assert_eq!(
            notice_text("WhatsApp", Vec::new()),
            ("WhatsApp".to_string(), String::new())
        );
    }

    #[test]
    fn source_falls_back_to_aumid_then_generic() {
        assert_eq!(
            source_name(Some("WhatsApp".into()), Some("5319.WhatsApp!App".into())),
            "WhatsApp"
        );
        assert_eq!(
            source_name(None, Some("5319.WhatsApp!App".into())),
            "5319.WhatsApp!App"
        );
        assert_eq!(source_name(Some(String::new()), Some("Id".into())), "Id");
        assert_eq!(source_name(None, None), "Notifikasi");
        assert_eq!(source_name(None, Some(String::new())), "Notifikasi");
    }

    #[test]
    fn logos_fit_48_keeping_proportions_and_are_never_enlarged() {
        assert_eq!(fit_logo(48, 48), (48, 48));
        assert_eq!(fit_logo(16, 16), (16, 16));
        assert_eq!(fit_logo(96, 96), (48, 48));
        assert_eq!(fit_logo(256, 128), (48, 24));
        assert_eq!(fit_logo(100, 400), (12, 48));
        assert_eq!(fit_logo(1, 1000), (1, 48), "never below one pixel");
        assert_eq!(fit_logo(0, 0), (1, 1));
        for (w, h) in [(49, 7), (300, 299), (4096, 2)] {
            let (tw, th) = fit_logo(w, h);
            assert!((1..=48).contains(&tw) && (1..=48).contains(&th), "{w}x{h}");
        }
    }

    #[test]
    fn bytes_that_are_no_image_give_no_logo() {
        // WIC is not asked to decode anything real here: garbage and nothing.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        assert_eq!(decode_logo(&[]), None);
        assert_eq!(decode_logo(b"definitely not an image"), None);
    }

    fn split(app_id: Option<&str>, source: &str, items: &[&str]) -> (Vec<String>, Option<String>) {
        split_site(app_id, source, texts(items))
    }

    #[test]
    fn chrome_toast_gives_its_site() {
        let (rest, site) = split(
            Some("Chrome"),
            "Chrome",
            &["Rani", "Halo", "web.telegram.org"],
        );
        assert_eq!(site.as_deref(), Some("web.telegram.org"));
        let (title, body) = notice_text("Chrome", rest);
        assert_eq!((title.as_str(), body.as_str()), ("Rani", "Halo"));
    }

    #[test]
    fn edge_via_prefix_is_dropped_from_the_site() {
        let (rest, site) = split(
            Some("MSEdge"),
            "Microsoft Edge",
            &["Judul", "isi", "via www.example.com"],
        );
        assert_eq!(site.as_deref(), Some("www.example.com"));
        assert_eq!(rest, texts(&["Judul", "isi"]));
        let (_, site) = split(Some("MSEdge"), "x", &["T", "b", "VIA www.example.com"]);
        assert_eq!(site.as_deref(), Some("www.example.com"));
    }

    #[test]
    fn browser_is_found_by_display_name_alone() {
        let (rest, site) = split(None, "Google Chrome", &["Rani", "Halo", "web.telegram.org"]);
        assert_eq!(site.as_deref(), Some("web.telegram.org"));
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn firefox_matches_by_display_name() {
        let (_, site) = split(
            Some("308046B0AF4A39CB"),
            "Firefox",
            &["T", "b", "mail.example.org"],
        );
        assert_eq!(site.as_deref(), Some("mail.example.org"));
    }

    #[test]
    fn host_with_a_port_is_a_site() {
        for host in ["localhost.test:8080", "192.168.1.10:3000"] {
            let (_, site) = split(Some("Chrome"), "Chrome", &["T", "b", host]);
            assert_eq!(site.as_deref(), Some(host));
        }
        let (_, site) = split(Some("Chrome"), "Chrome", &["T", "b", "via a.test:80"]);
        assert_eq!(site.as_deref(), Some("a.test:80"));
    }

    #[test]
    fn non_browser_keeps_its_host_line_in_the_body() {
        let (rest, site) = split(
            Some("TelegramMessengerLLP.TelegramDesktop"),
            "Telegram",
            &["Rani", "Halo", "web.telegram.org"],
        );
        assert_eq!(site, None);
        assert_eq!(rest, texts(&["Rani", "Halo", "web.telegram.org"]));
        let (_, body) = notice_text("Telegram", rest);
        assert_eq!(body, "Halo · web.telegram.org");
    }

    #[test]
    fn text_with_spaces_is_no_host() {
        let (rest, site) = split(Some("Chrome"), "Chrome", &["T", "see example.com now"]);
        assert_eq!(site, None);
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn single_element_browser_toast_has_no_site() {
        let (rest, site) = split(Some("Chrome"), "Chrome", &["web.telegram.org"]);
        assert_eq!(site, None);
        assert_eq!(rest, texts(&["web.telegram.org"]));
    }

    #[test]
    fn via_chrome_without_a_dot_is_no_host() {
        let (_, site) = split(Some("Chrome"), "Chrome", &["Ibu", "halo", "via Chrome"]);
        assert_eq!(site, None);
    }

    #[test]
    fn bare_host_rules() {
        assert_eq!(bare_host(" a.b "), Some("a.b".to_string()));
        for bad in [
            "", "via", "via ", "localhost", ".a.b", "a.b.", "-a.b", "a.b-", "a..b", "a.b:",
            "a.b:123456", "a.b:80x", "a.b:8:9", "a_b.c", "\u{e9}.com", "a b.c",
        ] {
            assert_eq!(bare_host(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn browser_names_are_whole_words() {
        assert!(is_browser(Some("MSEdge"), "x"));
        assert!(is_browser(Some("Brave.Browser"), "x"));
        assert!(is_browser(None, "Microsoft Edge"));
        assert!(is_browser(None, "Opera GX"));
        assert!(is_browser(None, "Vivaldi"));
        assert!(!is_browser(None, "Knowledge Base"));
        assert!(!is_browser(Some("com.squirrel.slack.slack"), "Slack"));
        assert!(!is_browser(None, "Operator"));
    }

    #[test]
    fn muted_match_is_case_insensitive_substring() {
        let m = vec!["Claude".to_string(), "Orca".to_string()];
        assert!(is_muted("Claude", &m));
        assert!(is_muted("orca agent", &m));
        assert!(!is_muted("WhatsApp", &m));
    }
}
