---
tags: [decision, settings, tray, windows, multi-monitor, config]
date: 2026-11-02
---

# Notch monitor: which screen the island shows on

Before this the island always sat top-centre on the primary monitor. Now a
choice (Settings -> Tampilan -> Notch -> Monitor, or the tray's Monitor submenu)
puts it on any connected screen. Related: [[notch-size-and-fill]] (the card it
opens), [[settings-structure]] (the row list), [[arrange]] (`arrange_win`,
`Monitor`, the overlay `Canvas`), [[notch-peek-eyes]] and [[stay-behind]] (the
pointer and fullscreen logic that has to follow the window; another task owns
those changes).

## Config

`notch_monitor` (island-owned, in `merge_owned`): the JSON string `"primary"`
(default) or `{"path": "...", "name": "..."}`. `NotchMonitor::{Primary,
Chosen{path,name}}`. Reading never fails: missing, null, a non-object, an
unknown string, an object without a non-empty string `path` or wrong types are
Primary; a missing or non-string `name` is "". An old file without the key
loads as Primary.

## Identify by device path

`\\.\DISPLAYn` is renumbered by Windows, so a monitor is saved by its device
path (`DISPLAYCONFIG_TARGET_DEVICE_NAME.monitorDevicePath`), with the friendly
name kept only to show while it is unplugged. Two screens of one model share a
friendly name but never a path. `arrange_win::displays()` (EnumDisplayMonitors,
sorted left to right then top to bottom like `monitors()`, which is now
`displays()` mapped to `Display::monitor`) fills `Display { monitor, hmonitor,
primary, path, name, number }`. Paths come from `GetDisplayConfigBufferSizes` +
`QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)`; per path `DisplayConfigGetDeviceInfo`
GET_SOURCE_NAME gives the GDI name (matched to `szDevice`, case-insensitive) and
GET_TARGET_NAME the path and friendly name. With cloned outputs the first
target per source wins. Any failure leaves path and name empty. Cargo feature
`Win32_Devices_Display` (Cargo.lock unchanged).

**Assumption:** a display with an empty path is listed but cannot be chosen
(there is nothing to find it by later): it is greyed in Settings and the tray
and takes no click. `display_name` falls back to "Monitor N".

## Resolution (pure, `src/notch_monitor.rs`)

`resolve(choice, list)`: Primary gives the primary (else index 0); Chosen gives
the display whose path equals the saved one (case-insensitive); not connected
gives the primary and **the choice is not changed or saved**, so the island
returns when the screen does; an empty list gives `None` (the window then asks
`MonitorFromPoint(0,0, PRIMARY)` like before). `saved_missing`, `moved`
(rect, dpi or path differs), `resolve_paths` (the Settings layout only has
paths and primary flags).

## Placement and hot-plug

`window::place_on(rect, dpi, tall)` is pure and tested (top-centre, size
WINDOW_W/H x dpi/96, `screen_w` = monitor width / scale). `IslandWindow::create`
and `reposition` take `Option<&Display>`; None is the old primary path. The DPI
is still read from the monitor, never `GetDpiForWindow`.

`reposition()` (app.rs): take the window and a clone of the choice in the first
`with_app`, list `displays()` outside the borrow, resolve, `win.reposition`,
put the window back, store `App.placed` and `App.displays`, `refresh`,
`sync_settings`. `WM_DPICHANGED` / `WM_DISPLAYCHANGE` still call it.
`WM_DEVICECHANGE` with `DBT_DEVNODES_CHANGED` arms a one-shot `TIMER_DISPLAYS`
(41, 600 ms; Windows sends it in bursts); the timer re-lists and calls
`reposition()` only when `moved(placed, wanted)`, then `sync_settings`. No
polling. The existing `devices_page::on_device_change` call stays outside
`with_app`.

The setting changes live: `SettingsAction::NotchMonitor` runs inside
`with_app`, so it sets the config, saves, and sets `DEFERRED_REPOSITION`;
`run_deferred` (end of every window procedure call) does it, and again right
after the deferred-settings loop so an action that was itself deferred also
moves at once. The tray calls `set_notch_monitor` then `reposition()` outside
`with_app`.

Already follows the window (verified by reading, not changed): the lyric cap
(`win.screen_w()`), Susun's `monitor_index_at` at the notch centre,
`win.to_screen` users (editors, popovers, the inline capture strip). Not
touched: the Settings window's own placement (`primary_*` in settings.rs) and
the `--demo-editor` anchor. The pointer code (eyes, `near_top`), fullscreen
check and `follow_behind` belong to the parallel notch-monitor-follow task.

## Settings row

The Notch card gets a first block: "Monitor" with a description and the
"Tandai" button, a miniature map (`Control::MonitorMap`, every screen scaled to
fit about 96 dip high, real arrangement, number centred, the resolved screen
accent-filled and outlined; each rectangle is `Hit::NotchMonitor(Entry(i))`),
then a radio list (`Control::Radio`, 32 dip rows): "Monitor utama" with
"sekarang: N", one row per screen ("N · name" with "WxH", ", utama" on the
primary), the saved screen with "tidak terhubung" when absent (stays selected,
not clickable), and a muted "Cuma satu monitor terdeteksi" for one screen. The
card is 52 + map (104) + 32 per entry + padding taller than before; the five old
rows follow unchanged. `Model.monitors` is filled by the island from
`App.displays`; `Model::from_config` leaves it empty. A click posts only on a
change (`monitor_choice`).

## Tray

A "Monitor" submenu after Gaya: Pill / Notch: "Monitor utama", "N · name" per
screen (ids 100, 101 + i), radio-checked (`CheckMenuRadioItem`), greyed for a
screen without a path, plus the saved one as "name (tidak terhubung)" greyed
and checked. Built from the config and a fresh `displays()` every time the menu
opens; `show_menu` returns `MenuChoice::{Cmd, Monitor(NotchMonitor)}`.

## Identify overlay (`src/identify_overlay.rs`)

One small layered popup per screen (class `ValeIdentify`, in
`capture::win::KEPT_OUT`): a 160 dip dark plate (#1B1D1F, 90 %, radius 24) with
the number 96 dip in white, centred on the screen, never full-screen so the
shell does not read it as fullscreen. `WS_EX_LAYERED | TRANSPARENT | NOACTIVATE
| TOPMOST | TOOLWINDOW`, `SW_SHOWNOACTIVATE`, drawn with the `Canvas` of
`arrange_overlay` (now `pub(crate)`, with `paint_with` / `present`). Hold 2 s on a
one-shot timer of the first plate; with fades (`motion().moves()` and Windows
animations) 120 ms in and 200 ms out on a 16 ms timer that exists only during a
fade. A second `show` restarts the hold and refreshes numbers and places. The
window procedure never calls `with_app`.

## Debug and demo

- `--settings`: `DI_MONITORS=1` (one 1920x1080), `3` (three side by side, the
  middle one primary, two with the same name), `3-missing` (the same plus a
  saved screen that is not connected); unset lists the real displays. "Tandai"
  shows the plates on the real screens.
- `--demo*`: `DI_NOTCH_MONITOR=N` puts the demo island on display N (the
  primary when unset or invalid).

## Verified and not verified live

Verified: unit tests (config, resolve, place_on, layout, tray, fades), a
`--settings` run with `DI_MONITORS=1|3|3-missing` in dark and light (screenshots
looked at), and "Tandai" on this PC's two real screens (plates seen on both,
gone after 2 s, no stray window left).

Not verified live: a real hot-plug (unplugging or plugging a screen while Vale
runs), the island actually moving to a second screen of a different DPI from the
tray or Settings, a path-less display, the fade look at 120 / 200 ms, and the
tray submenu with a real mouse (only its item model is tested).
