use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use crate::i18n::{self, Key, tr};
use crate::model::{Notice, NoticeKind};

/// `BatteryLifePercent` value meaning the charge level is unknown.
const PERCENT_UNKNOWN: u8 = 255;
/// `BatteryFlag` bit set when the machine has no system battery.
const FLAG_NO_BATTERY: u8 = 128;
/// `ACLineStatus` value meaning the charger is connected.
const AC_ONLINE: u8 = 1;
/// `SystemStatusFlag` value meaning Energy Saver (battery saver) is on.
const SAVER_ON: u8 = 1;

/// Decides when to raise the low-battery notice: once per discharge below the
/// threshold, re-armed by charging or by rising back above the threshold.
pub struct BatteryMonitor {
    threshold: u8,
    alerted: bool,
}

impl BatteryMonitor {
    pub fn new(threshold: u8) -> Self {
        Self {
            threshold,
            alerted: false,
        }
    }

    /// A new threshold from the settings window; an alert already raised
    /// this discharge stays raised.
    pub fn set_threshold(&mut self, threshold: u8) {
        self.threshold = threshold;
    }

    /// True when the alert should be raised now.
    pub fn update(&mut self, percent: u8, charging: bool) -> bool {
        if charging || percent > self.threshold {
            self.alerted = false;
            return false;
        }
        if self.alerted {
            return false;
        }
        self.alerted = true;
        true
    }
}

/// `BatteryFlag` value meaning the status could not be read.
const FLAG_UNKNOWN: u8 = 255;

/// Whether `BatteryFlag` says there is a battery: bit 128 means none; 255
/// (unknown) counts as one, so a stepper that needs it is not switched off
/// on a guess.
fn flag_has_battery(flag: u8) -> bool {
    flag == FLAG_UNKNOWN || flag & FLAG_NO_BATTERY == 0
}

/// This PC has a battery (Settings → Umum → Peringatan): the power status
/// says so, or cannot be read at all.
pub fn battery_present() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    match unsafe { GetSystemPowerStatus(&mut status) } {
        Ok(()) => flag_has_battery(status.BatteryFlag),
        Err(_) => true,
    }
}

/// Current charge percentage and whether the charger is connected, or `None`
/// when there is no battery or its level is unknown.
pub fn read_battery() -> Option<(u8, bool)> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    if status.BatteryLifePercent == PERCENT_UNKNOWN || status.BatteryFlag & FLAG_NO_BATTERY != 0 {
        return None;
    }
    Some((status.BatteryLifePercent, status.ACLineStatus == AC_ONLINE))
}

/// Windows' Energy Saver is on: the robot stops blinking and moving.
pub fn energy_saver_on() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && saver_flag(status.SystemStatusFlag)
}

fn saver_flag(flag: u8) -> bool {
    flag == SAVER_ON
}

/// At or below `threshold` and not charging: the idle robot looks low.
pub fn battery_low_now(reading: Option<(u8, bool)>, threshold: u8) -> bool {
    reading.is_some_and(|(percent, charging)| !charging && percent <= threshold)
}

pub fn battery_notice(percent: u8) -> Notice {
    Notice {
        key: "battery".to_string(),
        source: tr(Key::SourceBattery).to_string(),
        title: tr(Key::BatteryLow).to_string(),
        body: i18n::battery_body(i18n::lang(), percent),
        kind: NoticeKind::BatteryLow,
    }
}

/// Diagnostic mode for `--probe-system`: prints the battery and fullscreen
/// state every second until Ctrl+C.
#[cfg(debug_assertions)]
pub fn probe() {
    println!("Polling battery and fullscreen state; press Ctrl+C to stop.");
    loop {
        println!(
            "battery={:?} fullscreen={}",
            read_battery(),
            super::fullscreen::is_fullscreen(None)
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_no_battery_flag_switches_the_stepper_off() {
        assert!(!flag_has_battery(128), "no system battery");
        assert!(flag_has_battery(255), "unknown counts as present");
        for flag in [0, 1, 2, 4, 8, 9] {
            assert!(flag_has_battery(flag), "{flag}");
        }
        // 128 with other bits (charging, low) is still "no battery".
        assert!(!flag_has_battery(128 | 8));
    }

    #[test]
    fn alerts_once_when_crossing() {
        let mut b = BatteryMonitor::new(20);
        assert!(!b.update(25, false));
        assert!(b.update(20, false));
        assert!(!b.update(19, false));
    }

    #[test]
    fn charging_never_alerts_and_rearms() {
        let mut b = BatteryMonitor::new(20);
        assert!(b.update(18, false));
        assert!(!b.update(18, true));
        assert!(b.update(18, false));
    }

    #[test]
    fn rearms_after_rising_above_threshold() {
        let mut b = BatteryMonitor::new(20);
        assert!(b.update(19, false));
        assert!(!b.update(30, false));
        assert!(b.update(20, false));
    }

    #[test]
    fn low_only_while_discharging_at_the_threshold() {
        assert!(battery_low_now(Some((20, false)), 20));
        assert!(battery_low_now(Some((5, false)), 20));
        assert!(!battery_low_now(Some((21, false)), 20));
        assert!(!battery_low_now(Some((5, true)), 20), "charging");
        assert!(!battery_low_now(None, 20), "no battery");
        assert!(saver_flag(1) && !saver_flag(0));
    }

    #[test]
    fn notice_text() {
        let n = battery_notice(15);
        assert_eq!(n.key, "battery");
        assert_eq!(n.source, "Baterai");
        assert_eq!(n.title, "Baterai lemah");
        assert_eq!(n.body, "Sisa 15%. Colokin charger.");
        assert_eq!(n.kind, NoticeKind::BatteryLow);
    }
}
