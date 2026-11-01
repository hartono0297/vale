//! The island's side of the Devices page: the drives as last listed, which
//! are being ejected, the refusals, the card expanded and the scroll; the
//! worker that scans and ejects (`devices_win`); the change notifications
//! that start a scan; the notices; its hotkey press. A child of `app`, so
//! it works on `App` directly. The wording and the arithmetic are
//! `devices.rs`, the layout is `panel_layout::devices_layout`.
//!
//! Power: there is no repeating timer and no polling. A scan runs once at
//! startup, once when the page opens (to read the sizes) and, after the
//! system says a volume or disk came or went, once more after a 400 ms
//! pause (`TIMER_DEVICES`, a one-shot every message restarts). The worker
//! thread blocks on its channel in between.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::time::Instant;

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Ioctl::GUID_DEVINTERFACE_DISK;
use windows::Win32::UI::WindowsAndMessaging::{
    DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVTYP_DEVICEINTERFACE, DBT_DEVTYP_VOLUME,
    DEV_BROADCAST_DEVICEINTERFACE_W, DEV_BROADCAST_HDR, DEVICE_NOTIFY_WINDOW_HANDLE, HDEVNOTIFY,
    KillTimer, RegisterDeviceNotificationW, SetTimer, UnregisterDeviceNotification,
};

use super::{App, poster};
use crate::devices::{self, Change, Device, DevicesEvent, Ejected, Veto};
use crate::devices_win::{Capacity, Devices, Req};
use crate::i18n::{self, Lang};
use crate::model::{Event, Page};
use crate::shortcuts;
use crate::ui::geometry::{ShapeKind, ShapeRect, shape_for};
use crate::ui::panel_layout::{DEV_WHEEL_PX, device_shapes, devices_max_scroll, devices_reveal};

/// One-shot: the pause after the last device change message, then a scan
/// (`Capacity::NewOnly`). Every relevant message starts it again.
pub(super) const TIMER_DEVICES: usize = 32;
const DEBOUNCE_MS: u32 = 400;
/// What `WM_DEVICECHANGE` reports in `wparam`.
const ARRIVAL: usize = DBT_DEVICEARRIVAL as usize;
const REMOVED: usize = DBT_DEVICEREMOVECOMPLETE as usize;

/// What the Devices page keeps: the worker (started by the first request),
/// the list as last read, and the page's own state.
#[derive(Default)]
pub(super) struct DevicesState {
    worker: Option<Devices>,
    /// The drives as the last scan found them.
    pub list: Vec<Device>,
    /// The first scan (at startup) came back: later arrivals get a notice.
    seeded: bool,
    /// Scans asked for so far, and the newest one applied (an older answer
    /// is dropped).
    seq: u64,
    applied: u64,
    /// Keys being ejected right now (their buttons read "Melepas…").
    pub ejecting: HashSet<String>,
    /// The last eject Windows refused, per key, until the next try.
    pub vetoes: HashMap<String, Veto>,
    /// The key of the card whose details show; one at a time.
    pub expanded: Option<String>,
    /// How far the list is scrolled (px).
    pub scroll: f32,
    /// The page showed at the last `sync_devices`.
    shown: bool,
    /// The keys in the order the last frame drew them: a click's index means
    /// the card the user saw.
    drawn: Vec<String>,
    /// The disk-interface notification (`HDEVNOTIFY`), 0 without one.
    notify: isize,
}

/// A `WM_DEVICECHANGE` that may mean a drive came or went: an arrival or a
/// completed removal of a volume or of a disk interface. The node-list
/// changes Windows sends in bursts (`DBT_DEVNODES_CHANGED`) and everything
/// else are not.
pub(super) fn relevant_device_change(wparam: usize, device_type: Option<u32>) -> bool {
    matches!(wparam, ARRIVAL | REMOVED)
        && matches!(
            device_type,
            Some(t) if t == DBT_DEVTYP_VOLUME.0 || t == DBT_DEVTYP_DEVICEINTERFACE.0
        )
}

/// `WM_DEVICECHANGE`: an arrival or a removal of a volume or disk starts (or
/// restarts) the pause before the scan. `lparam` is a `DEV_BROADCAST_HDR` for
/// those messages. Needs no `App`: the system sends these synchronously, so
/// one can arrive while a handler holds `App`, and the timer is armed anyway.
pub(super) fn on_device_change(hwnd: HWND, wparam: usize, lparam: isize) {
    // Only the two messages that carry a header have one to read: for the
    // others `lparam` is not a pointer (or not ours to follow).
    let device_type = (matches!(wparam, ARRIVAL | REMOVED) && lparam != 0).then(|| {
        let header = lparam as *const DEV_BROADCAST_HDR;
        unsafe { (*header).dbch_devicetype.0 }
    });
    if relevant_device_change(wparam, device_type) {
        unsafe {
            SetTimer(Some(hwnd), TIMER_DEVICES, DEBOUNCE_MS, None);
        }
    }
}

/// The list's scroll after a wheel turn of `delta` (120 a notch, positive
/// away from the user: up), kept in `0..=max`.
pub(super) fn wheel_scroll(scroll: f32, delta: i32, max: f32) -> f32 {
    (scroll - delta as f32 / 120.0 * DEV_WHEEL_PX).clamp(0.0, max.max(0.0))
}

/// The keys "Eject semua" asks for: every listed device, but none while
/// anything is being ejected (the button is off then).
pub(super) fn eject_all_keys(list: &[Device], ejecting: &HashSet<String>) -> Vec<String> {
    if !ejecting.is_empty() {
        return Vec::new();
    }
    list.iter().map(|d| d.key.clone()).collect()
}

impl DevicesState {
    /// A scan came back. `None` when it is older than one already applied;
    /// else the change from the list before it (arrivals are only reported
    /// once the startup scan has seeded the list). What left the list loses
    /// its refusal, its ejecting mark and its expanded card, and the scroll
    /// stays in range.
    fn accept(&mut self, seq: u64, devices: Vec<Device>) -> Option<Change> {
        if seq < self.applied {
            return None;
        }
        self.applied = seq;
        let change = devices::diff(&self.list, &devices);
        for key in &change.removed {
            self.vetoes.remove(key);
            self.ejecting.remove(key);
            if self.expanded.as_deref() == Some(key.as_str()) {
                self.expanded = None;
            }
        }
        self.list = devices;
        let first = !std::mem::replace(&mut self.seeded, true);
        Some(if first {
            Change {
                arrived: Vec::new(),
                ..change
            }
        } else {
            change
        })
    }

    /// An eject finished: the names that were safely ejected (for the notice);
    /// a refusal is kept for its card, a success drops what was kept. A
    /// device that was gone already clears its card but is not named, and a
    /// safe one the worker had no name for is named from `list` or not at
    /// all: an instance ID is never a name.
    fn finish_eject(
        &mut self,
        lang: Lang,
        results: Vec<(String, Option<String>, Result<Ejected, Veto>)>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        for (key, name, outcome) in results {
            self.ejecting.remove(&key);
            match outcome {
                Ok(Ejected::Gone) => {
                    self.vetoes.remove(&key);
                }
                Ok(Ejected::Safe) => {
                    self.vetoes.remove(&key);
                    let named = name.or_else(|| {
                        self.list
                            .iter()
                            .find(|d| d.key == key)
                            .map(|d| devices::display_name(lang, d))
                    });
                    names.extend(named);
                }
                Err(veto) => {
                    self.vetoes.insert(key, veto);
                }
            }
        }
        names
    }
}

impl App {
    /// At startup: the change notification and the seed scan (no
    /// sizes read, no notices).
    pub(super) fn devices_start(&mut self) {
        self.register_device_notify();
        self.devices_scan(Capacity::Keep);
    }

    /// Sends `req`, starting the worker on the first one. False when the
    /// thread is gone (the next request starts it again).
    fn devices_request(&mut self, req: Req) -> bool {
        let state = &mut self.devices;
        if state.worker.is_none() {
            state.worker = Devices::spawn(self.tx.clone(), poster(self.hwnd, super::WM_APP_EVENT));
        }
        let sent = state.worker.as_ref().is_some_and(|w| w.request(req));
        if !sent {
            state.worker = None;
        }
        sent
    }

    fn devices_scan(&mut self, capacity: Capacity) {
        self.devices.seq += 1;
        let seq = self.devices.seq;
        self.devices_request(Req::Scan { seq, capacity });
    }

    /// Asks the system for the disk-interface arrivals and removals, on top
    /// of the volume broadcasts every top-level window gets.
    fn register_device_notify(&mut self) {
        let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_classguid: GUID_DEVINTERFACE_DISK,
            ..Default::default()
        };
        let note = unsafe {
            RegisterDeviceNotificationW(
                HANDLE(self.hwnd.0),
                (&filter as *const DEV_BROADCAST_DEVICEINTERFACE_W).cast::<c_void>(),
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        };
        if let Ok(note) = note {
            self.devices.notify = note.0 as isize;
        }
    }

    /// `TIMER_DEVICES`: the pause is over; scan, reading the size of what is
    /// new only.
    pub(super) fn on_devices_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_DEVICES);
        }
        self.devices_scan(Capacity::NewOnly);
    }

    /// The worker's answer. A list: arrivals get their notice (switchable in
    /// Settings), and the open page redraws when something changed. An
    /// eject: the names that went get one "Aman dicabut", a refusal lands on
    /// its card.
    pub(super) fn on_devices_event(&mut self, event: DevicesEvent, now: Instant) {
        let changed = match event {
            DevicesEvent::Listed { seq, devices } => {
                let was = self.devices.list.clone();
                let Some(change) = self.devices.accept(seq, devices) else {
                    return;
                };
                if self.config.device_notices {
                    let lang = i18n::lang();
                    for device in &change.arrived {
                        self.raise_device_notice(devices::plugged_notice(lang, device), now);
                    }
                }
                was != self.devices.list || !change.removed.is_empty()
            }
            DevicesEvent::Ejected { results } => {
                let names = self.devices.finish_eject(i18n::lang(), results);
                if self.config.device_notices && !names.is_empty() {
                    self.raise_device_notice(devices::safe_notice(i18n::lang(), &names), now);
                }
                true
            }
        };
        let shown = self.devices_page_shown();
        if shown {
            self.devices.scroll = self.devices.scroll.min(self.devices_max_scroll());
        }
        if changed && shown && !self.animating() {
            self.draw(now);
        }
    }

    /// A notice from the worker's answer: the drain refreshes the island for
    /// it (`raised`).
    fn raise_device_notice(&mut self, notice: crate::model::Notice, now: Instant) {
        self.system.raised = true;
        self.apply(Event::Notice(notice), now);
    }

    /// The Devices page is what the open panel shows (a notice over it
    /// does not count).
    fn devices_page_shown(&self) -> bool {
        self.state.page() == Page::Devices && self.state.panel_is_open() && !self.state.fullscreen()
    }

    /// The Devices page just opened: one scan reading every size, and the
    /// page starts folded and at the top. It leaving does nothing (no timer
    /// ran); what it showed stays for its closing fade.
    pub(super) fn sync_devices(&mut self) {
        let wanted = self.devices_page_shown();
        if wanted && !self.devices.shown {
            self.devices.expanded = None;
            self.devices.scroll = 0.0;
            self.devices_scan(Capacity::All);
        }
        self.devices.shown = wanted;
    }

    /// The panel's shape and whether the waiting strip takes room: what the
    /// list's room depends on.
    fn devices_geometry(&self) -> (ShapeRect, bool) {
        (
            shape_for(self.config.style, ShapeKind::Panel),
            self.state.waiting_strip().is_some(),
        )
    }

    fn devices_max_scroll(&self) -> f32 {
        let (shape, strip) = self.devices_geometry();
        let shapes = device_shapes(
            &self.devices.list,
            &self.devices.vetoes,
            self.devices.expanded.as_deref(),
        );
        devices_max_scroll(shape, strip, &shapes)
    }

    /// Remembers the keys of the cards this frame draws, before it draws.
    pub(super) fn devices_sync_drawn(&mut self) {
        let same = self.devices.drawn.iter().map(String::as_str).eq(self
            .devices
            .list
            .iter()
            .map(|d| d.key.as_str()));
        if !same {
            self.devices.drawn = self.devices.list.iter().map(|d| d.key.clone()).collect();
        }
    }

    /// The card `i` of the frame the user saw, from the list as it is now
    /// (`None` when that device has gone).
    fn devices_card(&self, i: usize) -> Option<&Device> {
        let key = self.devices.drawn.get(i)?;
        self.devices.list.iter().find(|d| &d.key == key)
    }

    /// Eject, or Coba lagi: the device is marked as ejecting and its old
    /// refusal goes; the answer comes back as `DevicesEvent::Ejected`.
    pub(super) fn devices_eject(&mut self, i: usize) {
        let Some(key) = self.devices_card(i).map(|d| d.key.clone()) else {
            return;
        };
        if self.devices.ejecting.contains(&key) {
            return;
        }
        self.devices.vetoes.remove(&key);
        self.devices.ejecting.insert(key.clone());
        if !self.devices_request(Req::Eject {
            keys: vec![key.clone()],
        }) {
            self.devices.ejecting.remove(&key);
        }
    }

    /// Eject semua: every listed device one after another on the worker; not
    /// while anything is being ejected.
    pub(super) fn devices_eject_all(&mut self) {
        let keys = eject_all_keys(&self.devices.list, &self.devices.ejecting);
        if keys.is_empty() {
            return;
        }
        for key in &keys {
            self.devices.vetoes.remove(key);
            self.devices.ejecting.insert(key.clone());
        }
        if !self.devices_request(Req::Eject { keys: keys.clone() }) {
            for key in &keys {
                self.devices.ejecting.remove(key);
            }
        }
    }

    /// The info button: this card's details unfold in place (one card at a
    /// time) and scroll into view, or fold again.
    pub(super) fn devices_toggle_detail(&mut self, i: usize) {
        let Some(key) = self.devices_card(i).map(|d| d.key.clone()) else {
            return;
        };
        if self.devices.expanded.as_deref() == Some(key.as_str()) {
            self.devices.expanded = None;
        } else {
            self.devices.expanded = Some(key.clone());
        }
        let (shape, strip) = self.devices_geometry();
        let shapes = device_shapes(
            &self.devices.list,
            &self.devices.vetoes,
            self.devices.expanded.as_deref(),
        );
        let index = self.devices.list.iter().position(|d| d.key == key);
        self.devices.scroll = match (self.devices.expanded.is_some(), index) {
            (true, Some(at)) => devices_reveal(shape, strip, &shapes, self.devices.scroll, at),
            _ => self
                .devices
                .scroll
                .min(devices_max_scroll(shape, strip, &shapes)),
        };
    }

    /// The folder button: Explorer on the device's first drive, after the
    /// panel has collapsed.
    pub(super) fn devices_open(&mut self, i: usize) {
        let letter = self
            .devices_card(i)
            .and_then(|d| d.partitions.first())
            .map(|p| p.letter);
        if let Some(letter) = letter {
            self.queue_shortcut(shortcuts::Action::Drive(letter));
        }
    }

    /// The wheel over the list scrolls it while it overflows (the notch is
    /// spent at either end); with everything fitting, `false`: the notch
    /// turns the page as anywhere else.
    pub(super) fn devices_wheel(&mut self, delta: i32) -> bool {
        let max = self.devices_max_scroll();
        if max <= 0.0 {
            return false;
        }
        let next = wheel_scroll(self.devices.scroll, delta, max);
        if next != self.devices.scroll {
            self.devices.scroll = next;
            if !self.animating() {
                self.draw(Instant::now());
            }
        }
        true
    }

    /// `WM_HOTKEY`: the page opens (or closes) without taking the
    /// foreground, held open like the Clipboard's.
    pub(super) fn on_devices_hotkey(&mut self) {
        self.apply_now(Event::DevicesHotkey);
        self.arm_hotkey_hold();
    }

    /// `WM_DESTROY`: the notification goes.
    pub(super) fn devices_shutdown(&mut self) {
        let note = std::mem::take(&mut self.devices.notify);
        if note != 0 {
            unsafe {
                let _ = UnregisterDeviceNotification(HDEVNOTIFY(note as *mut c_void));
            }
        }
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_DEVICES);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{Bus, DeviceKind, Partition, VetoKind};

    fn dev(key: &str, name: &str) -> Device {
        Device {
            key: key.into(),
            name: name.into(),
            model: name.into(),
            kind: DeviceKind::Usb,
            bus: Bus::Usb,
            partitions: vec![Partition {
                letter: 'E',
                label: String::new(),
                fs: "exFAT".into(),
                total: 0,
                free: None,
            }],
        }
    }

    fn veto() -> Veto {
        Veto {
            kind: VetoKind::WindowsApp,
            name: "EXCEL.EXE".into(),
        }
    }

    #[test]
    fn only_arrivals_and_removals_of_volumes_and_disks_start_a_scan() {
        let vol = Some(DBT_DEVTYP_VOLUME.0);
        let iface = Some(DBT_DEVTYP_DEVICEINTERFACE.0);
        for w in [ARRIVAL, REMOVED] {
            assert!(relevant_device_change(w, vol));
            assert!(relevant_device_change(w, iface));
            // A port or a handle, or no header at all.
            assert!(!relevant_device_change(w, Some(3)));
            assert!(!relevant_device_change(w, Some(6)));
            assert!(!relevant_device_change(w, None));
        }
        // The burst of node-list changes, a query and the rest.
        assert!(!relevant_device_change(0x0007, vol));
        assert!(!relevant_device_change(0x8001, vol));
        assert!(!relevant_device_change(0x8003, iface));
        assert!(!relevant_device_change(0, None));
    }

    #[test]
    fn the_startup_scan_seeds_the_list_without_arrivals() {
        let mut s = DevicesState::default();
        let change = s.accept(1, vec![dev("A", "SanDisk")]).expect("applied");
        assert!(change.arrived.is_empty(), "no notice for what was there");
        assert_eq!(s.list.len(), 1);
        // The next one reports what is new.
        let change = s
            .accept(2, vec![dev("A", "SanDisk"), dev("B", "WD")])
            .expect("applied");
        assert_eq!(change.arrived, vec![dev("B", "WD")]);
        assert!(change.removed.is_empty());
    }

    #[test]
    fn an_older_answer_is_dropped_and_an_equal_one_kept() {
        let mut s = DevicesState::default();
        assert!(s.accept(5, vec![dev("A", "x")]).is_some());
        assert!(s.accept(4, vec![]).is_none(), "stale");
        assert_eq!(s.list.len(), 1);
        // The list after an eject carries the newest number asked for.
        assert!(s.accept(5, vec![]).is_some());
        assert!(s.list.is_empty());
    }

    #[test]
    fn what_left_the_list_loses_its_veto_its_mark_and_its_card() {
        let mut s = DevicesState::default();
        s.accept(1, vec![dev("A", "x"), dev("B", "y")]);
        s.vetoes.insert("A".into(), veto());
        s.ejecting.insert("A".into());
        s.ejecting.insert("B".into());
        s.expanded = Some("A".into());
        s.accept(2, vec![dev("B", "y")]);
        assert!(s.vetoes.is_empty());
        assert_eq!(s.ejecting, HashSet::from(["B".to_owned()]));
        assert_eq!(s.expanded, None);
        // A card that stays keeps its expanded details.
        s.expanded = Some("B".into());
        s.accept(3, vec![dev("B", "y")]);
        assert_eq!(s.expanded.as_deref(), Some("B"));
    }

    #[test]
    fn an_eject_keeps_the_refusals_and_names_the_successes() {
        let mut s = DevicesState::default();
        s.accept(1, vec![dev("A", "SanDisk"), dev("B", "WD")]);
        s.ejecting.insert("A".into());
        s.ejecting.insert("B".into());
        s.vetoes.insert(
            "A".into(),
            Veto {
                kind: VetoKind::Device,
                name: String::new(),
            },
        );
        let names = s.finish_eject(
            Lang::En,
            vec![
                ("A".into(), Some("SanDisk".into()), Ok(Ejected::Safe)),
                ("B".into(), Some("WD".into()), Err(veto())),
            ],
        );
        assert_eq!(names, vec!["SanDisk".to_owned()]);
        assert!(s.ejecting.is_empty());
        assert!(
            !s.vetoes.contains_key("A"),
            "a success drops the old refusal"
        );
        assert_eq!(s.vetoes.get("B"), Some(&veto()));
        // Nothing went: no names, so no notice.
        let none = s.finish_eject(Lang::En, vec![("B".into(), Some("WD".into()), Err(veto()))]);
        assert!(none.is_empty());
    }

    #[test]
    fn a_device_that_was_gone_clears_its_card_without_a_name() {
        let mut s = DevicesState::default();
        s.accept(1, vec![dev("A", "SanDisk")]);
        s.ejecting.insert("A".into());
        s.vetoes.insert("A".into(), veto());
        let names = s.finish_eject(
            Lang::En,
            vec![("A".into(), Some("SanDisk".into()), Ok(Ejected::Gone))],
        );
        assert!(names.is_empty());
        assert!(s.ejecting.is_empty());
        assert!(s.vetoes.is_empty());
    }

    #[test]
    fn a_safe_eject_without_a_worker_name_is_named_from_the_list_or_not_at_all() {
        let mut s = DevicesState::default();
        let known = dev(r"USBSTOR\DISK&VEN_TEST", "SanDisk");
        s.accept(1, vec![known.clone()]);
        let names = s.finish_eject(Lang::En, vec![(known.key.clone(), None, Ok(Ejected::Safe))]);
        assert_eq!(names, vec![devices::display_name(Lang::En, &known)]);
        assert!(names.iter().all(|n| *n != known.key));
        // Not in the list either: nothing to name, so nothing is pushed.
        let names = s.finish_eject(
            Lang::En,
            vec![(r"USBSTOR\DISK&VEN_OTHER".into(), None, Ok(Ejected::Safe))],
        );
        assert!(names.is_empty());
    }

    #[test]
    fn eject_all_asks_for_every_device_but_not_while_one_ejects() {
        let list = vec![dev("A", "x"), dev("B", "y")];
        assert_eq!(
            eject_all_keys(&list, &HashSet::new()),
            vec!["A".to_owned(), "B".to_owned()]
        );
        let busy = HashSet::from(["A".to_owned()]);
        assert!(eject_all_keys(&list, &busy).is_empty());
        assert!(eject_all_keys(&[], &HashSet::new()).is_empty());
    }

    #[test]
    fn the_wheel_scrolls_down_for_a_notch_toward_the_user_and_stops_at_the_ends() {
        let max = 100.0;
        assert_eq!(wheel_scroll(0.0, -120, max), DEV_WHEEL_PX);
        assert_eq!(wheel_scroll(DEV_WHEEL_PX, 120, max), 0.0);
        assert_eq!(wheel_scroll(0.0, 120, max), 0.0, "the top keeps it");
        assert_eq!(wheel_scroll(90.0, -240, max), max, "the end keeps it");
        // A touchpad's small deltas move it a little.
        assert!(wheel_scroll(0.0, -30, max) > 0.0);
        assert!(wheel_scroll(0.0, -30, max) < DEV_WHEEL_PX);
        assert_eq!(wheel_scroll(5.0, -120, 0.0), 0.0, "nothing to scroll");
    }
}
