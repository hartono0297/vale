---
tags: [decision, gotcha, devices, eject, timers, power, v3.5]
date: 2026-10-30
---

# Devices page (Perangkat, v3.5 step 2)

`Page::Devices`, between System and CLI (12 pages, [[panel-pages]]), after
`docs/design/rencana-v3.5.html` page 03. Pure data and wording in
`src/devices.rs`, Win32 and the worker in `src/devices_win.rs`, the page's
state in `src/app_devices.rs` (a child of `app`, like the shelf), layout in
`panel_layout::devices_layout`, drawing in `render_panel::paint_devices`.

## What is listed (and what is not)

- A volume with a **drive letter** whose disk sits under a device node the
  system calls removable (`DN_REMOVABLE`). **The node Eject asks is the first
  `DN_REMOVABLE` ancestor, starting with the disk itself.** Its instance ID is
  the device's `key` (several letters of one disk are one card). Internal disks
  have no removable ancestor and never show (checked live: four internal
  volumes, chains of 7 to 9 nodes, none removable).
- **But a hot-plug SATA/AHCI or PCIe port flags an internal disk
  `DN_REMOVABLE`** (why Windows' tray offers to eject it). Guards in `scan`:
  the disk holding the Windows directory and any disk with a page file in use
  are never listed (`pinned`), and an ATA, SATA, SAS or RAID
  disk with no USB, Thunderbolt, USB4 or SD node on its chain is internal
  (`internal_disk`). NVMe is deliberately not in that bus list: a Thunderbolt 3
  enclosure's chain is plain PCI nodes and would vanish.
- **Only a page file IN USE hides a disk** (decision). It is read from the
  registry value `ExistingPageFiles` (`live_page_file_letters`, once per
  scan): its NT paths (`\??\C:\pagefile.sys` and the like) are mapped to
  drive letters by `page_file_letters`, and an entry that maps to no letter is
  ignored (a missed page file is harmless, Windows vetoes that eject). A
  leftover `pagefile.sys` on an old system disk in an enclosure is therefore
  listed. Only when the registry read fails does the file itself decide: a
  live page file gives `INVALID_FILE_ATTRIBUTES` with `ERROR_SHARING_VIOLATION`
  (32, checked live on `C:`) and that alone counts (`page_file_present`); a
  readable file, access denied and not found all mean absent.
- **The worker sets `SetThreadErrorMode(SEM_FAILCRITICALERRORS |
  SEM_NOOPENFILEERRORBOX)`** at its start: without it an empty card slot can
  raise "There is no disk in the drive" and block the thread until dismissed.
- A device with no vendor, product or label has an empty `name`;
  `devices::display_name` names its kind when drawn, so a language change
  re-renders it.
- **No drive letter means not listed** (decision): Open needs a letter, so a
  disk without one, a card slot with no card (`ERROR_NOT_READY` from
  `GetVolumeInformationW`), phones (MTP, never a letter), network drives and
  optical drives are left out. A volume that cannot be read for another reason
  (locked, unformatted) is still listed with an empty file system, so it can be
  ejected.
- Kind from `devices::classify` (SD or MMC bus, a USB reader named like a
  card, removable USB = flash drive, the rest a disk). The USB speed is a
  best-effort probe (`IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2` on the
  parent hub's interface, the port from `CM_DRP_ADDRESS`); any failing step
  reads plain "USB". Thunderbolt and USB4 come from an ancestor's ID.
  **Not verified with a real USB drive** (none was plugged in): the removable
  ancestor walk, the speed probe and a real eject need hardware.

## Worker, no polling

- `Devices` is a parked thread like `system::Sampler` / `audio::Audio`:
  `Req::Scan { seq, capacity }` and `Req::Eject { keys }`, answers as
  `Event::Devices(DevicesEvent::Listed / Ejected)`. Queued scans merge (newest
  `seq`, widest capacity); an eject is followed by a fresh `Listed` carrying
  the newest scan number. The UI thread never calls these APIs.
- **Every handle is closed (`Handle` guard) before the request returns**: one
  kept open would veto our own eject. Volumes and disk interfaces are opened
  with access 0 and full sharing.
- **Eject looks the node up again** (`CM_Locate_DevNodeW(key)`) and passes a
  veto type and a 260-u16 name buffer to `CM_Request_Device_EjectW`, which also
  stops Windows from showing its own dialog. `CR_SUCCESS` with veto 0 is
  `Ejected::Safe`; veto 13 (`AlreadyRemoved`) and a key that no longer locates
  are `Ejected::Gone` (`devices::eject_outcome`): the card goes but nothing was
  ejected, so no "Aman dicabut". The notice names only what the worker knew
  (`Option` name from its last list) or the App knew from its list
  (`finish_eject`), never an instance ID; with no name there is no notice.
  Tests never eject anything real: only a made-up ID is looked up.
- **Debounce instead of polling.** `WM_DEVICECHANGE` on the island window
  (a top-level popup gets the volume broadcasts), plus
  `RegisterDeviceNotificationW` for `GUID_DEVINTERFACE_DISK`. Only
  `DBT_DEVICEARRIVAL` / `DBT_DEVICEREMOVECOMPLETE` with a volume (2) or device
  interface (5) header count, `DBT_DEVNODES_CHANGED` and the rest are ignored
  (the handler returns TRUE). The handler is a free function on the `hwnd`,
  not `with_app`: the broadcasts are sent synchronously and can arrive while
  `App` is borrowed, where `with_app` would drop them. Each one restarts a
  400 ms one-shot `TIMER_DEVICES` (32) that sends `Scan { NewOnly }`. No
  repeating timer anywhere ([[bata-idle-power-budget]]).
- **Capacity only when needed:** `GetDiskFreeSpaceExW` runs on the worker for
  every volume when the page opens (`All`), for a drive not in the last list
  (`NewOnly`, so its notice can say how much is free) and never for the
  startup seed (`Keep`, which carries the old figures). The scan after an
  eject is `NewOnly` too: a drive plugged in while the eject ran shows up
  there first, and the debounced scan after it would find it known and never
  read its size. The seed scan raises no notices (`DevicesState::accept`).

## Page, notices, hotkey

- Cards: icon well, title (`SanDisk Ultra · E:`, two spaces between letters),
  subtitle, folder / info / Eject buttons, a bar and `used / total` in mono
  (a dash before the size is known). Three cards with one refusal row fit the
  panel (tested); more cards or an unfolded card scroll with the wheel
  (`App::devices_wheel`, like the Mixer's rows: only while the list
  overflows). Unfolding scrolls the card into view
  (`panel_layout::devices_reveal`). Clicks map index to key through the keys of
  the last drawn frame (`devices_sync_drawn`), so a list that changed since
  the frame cannot redirect a click. Folding and the scroll reset when the page
  opens, not when it closes (the closing fade still draws what it showed).
- Refusals are worked out at draw time (`veto_message`), so a language change
  re-renders them. Only `WindowsApp` (3) names a program (`app_name`).
  `PendingClose` (2) and `OutstandingOpen` (5) carry a device instance path
  (`USBSTOR\DISK&VEN_...`, per the `PNP_VETO_TYPE` remarks), so both show the
  plain "still in use" line and never the name. Windows does not always name
  the program either.
- Notices (`DevicePlugged { letter }` with "Buka", `DeviceSafe`) are on by
  default and `config.device_notices` switches both off (Settings → Umum →
  Perangkat). The safe notice is raised only after an eject started from Vale.
  A click on the plugged notice collapses the panel and opens Explorer on the
  letter (`Effect::OpenDrive`). Bata: idle face for plugged, happy for safe.
- The tile `Go(Devices)` is **default-on** (decision): a fresh install shows
  twelve tiles; an old config with 11 shown gets it, one with 12 does not
  ([[shortcut-tiles]]).
- Optional hotkey: the `devices` row of `config.hotkeys` ([[hotkeys]], off by
  default; it was `devices_hotkey` Ctrl+Alt+D, migrated once),
  `RegisterHotKey` id 3. The Fitur "Perangkat" card is now one row (the
  notice toggle). The reducer's hold is generalised
  (`hotkey_held: Option<Page>`): the Clipboard and Devices hotkeys share one
  hold, one timer (`TIMER_CLIP_HOLD`) and every place that cleared the old
  `clip_held` clears it. The Clipboard hotkey behaves as before.
- Open mode and notices with a button: [[open-mode]]. Worker pattern:
  [[system-page]], [[mixer]]. Linger and motion: [[panel-linger-and-motion]].

## Gotchas

- **An empty instance ID is the root of the device tree** to
  `CM_Locate_DevNodeW`. `eject` refuses an empty key before any call, and only
  asks a node that is still `DN_REMOVABLE` (the root and internal nodes are
  not); `device_id` never returns an empty ID. (A first version of a test
  looked up `""` and Windows, luckily, vetoed the eject of the root.)
- `cargo test live_scan -- --ignored --nocapture` prints how this machine's
  volumes resolve (read-only); use `rtk bypass` to see the output.
- The capture script's `devices` shot (`--demo-devices`) needs the installed
  Vale closed: its cat ears show above the panel otherwise
  ([[screenshot-capture-script]]). Scenes use fixed made-up drives
  ([[demo-data-time-of-day]]).
- Many old tile and page tests counted 11 and 19; they now count 12 and 20.

Listed in [[index]].
