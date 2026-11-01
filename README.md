<p align="center"><img src="assets/logo-256.png" width="96" height="96" alt="Vale logo: a terracotta cat head with two ears and two dark pill eyes"></p>

<h1 align="center">Vale</h1>

<p align="center"><b>A notch for the top of your Windows 11 screen.</b><br>
Music with synced lyrics, calendar, clipboard history, volume mixer, file shelf, quick search and more, one hover away.</p>

<p align="center">
  <a href="https://github.com/hartono0297/dynamic-island/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/hartono0297/dynamic-island?style=flat-square&color=c8644a"></a>
  <a href="https://github.com/hartono0297/dynamic-island/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/hartono0297/dynamic-island/total?style=flat-square&color=c8644a"></a>
  <a href="https://github.com/hartono0297/dynamic-island/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/hartono0297/dynamic-island?style=flat-square&color=c8644a"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/github/license/hartono0297/dynamic-island?style=flat-square&color=c8644a"></a>
  <img alt="Windows 11" src="https://img.shields.io/badge/Windows-11-c8644a?style=flat-square">
  <img alt="Rust, about 2 MB, no runtime" src="https://img.shields.io/badge/Rust-2%20MB-c8644a?style=flat-square">
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#what-it-does">Features</a> ·
  <a href="#uninstall">Uninstall</a> ·
  <a href="docs/guide.md">Guide</a> ·
  <a href="#credits">Credits</a>
</p>

<p align="center"><img src="assets/screenshots/tour.png" width="560" alt="Animated tour of Vale's pages: Calendar, Pomodoro, Reminders, Notes, Music with lyrics, Mixer, Clipboard, File shelf, System, Devices, CLI, Shortcuts and Search"></p>

## Why Vale

- **Native and tiny.** One exe of about 2 MB, written in Rust (Win32 and Direct2D). No browser engine, no runtime to install.
- **Made for Windows 11.** It reads Windows' own media sessions, notifications, audio and Explorer, so it works with Spotify, music in the browser, Teams and the apps you already use.
- **Yours to shape.** Open it on hover, hover-and-stay or click; reorder or hide pages; pick the accent colour, the size of the closed notch (width and height), a solid or see-through fill and the island's background (it follows the cat, or one of five colours), your own shortcut tiles and hotkeys. Indonesian and English.
- **Private.** Notes, reminders, clipboard, notification history and files stay on your PC. No account, no telemetry.
- **The equalizer follows the music.** The five bars beside the song title move with what is actually playing (bass to treble), not a made-up wave. It listens only while the compact music view is on screen, and one switch in Settings turns it off.
- **A small cat lives in the notch.** Its face and ears tell you what is going on: listening to music, focused on a pomodoro, thinking while Claude Code works, asleep when you are away. It greets you by the time of day (morning, midday, afternoon, evening, late night), once per period. Pick its coat (Terracotta, Calico, White or Black) and an accessory (glasses, bow, nightcap, scarf, flower or crown).

## Install

Windows 11, 64-bit. Open **PowerShell** (no admin needed) and run:

```powershell
irm https://raw.githubusercontent.com/hartono0297/dynamic-island/main/install.ps1 | iex
```

The script downloads the latest [release](https://github.com/hartono0297/dynamic-island/releases/latest), checks its SHA-256, installs `vale.exe` to `%LOCALAPPDATA%\Programs\Vale`, adds a Start Menu shortcut and starts it. Vale then sits at the top centre of your screen, with an icon in the tray.

- **First run:** Windows asks for access to your notifications; allow it so toasts show in the notch.
- **Update:** run the same line again. Settings, notes and your Google login are kept.
- **Without the script:** download `vale-<version>.zip` from [Releases](https://github.com/hartono0297/dynamic-island/releases/latest), extract it, right-click `install.ps1` → *Run with PowerShell*. The exe also runs from any folder.
- **SmartScreen:** the exe is not code-signed yet. If Windows warns about it, choose *More info → Run anyway*.
- **Coming from Dynamic Island** (Vale's old name): installing Vale replaces it and moves your data to `%APPDATA%\Vale`.

## Uninstall

Open **PowerShell** and run:

```powershell
irm https://raw.githubusercontent.com/hartono0297/dynamic-island/main/uninstall.ps1 | iex
```

This quits Vale and removes its program folder, Start Menu shortcut, autostart entry and its Claude Code hooks. **Your data is kept** in `%APPDATA%\Vale` (settings, notes, reminders, clipboard and notification history, Google login), so a later install picks up where you left off.

To remove your data as well:

```powershell
$env:DI_PURGE = '1'; irm https://raw.githubusercontent.com/hartono0297/dynamic-island/main/uninstall.ps1 | iex
```

Offline, the copy of the script installed next to Vale does the same:

```powershell
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Vale\uninstall.ps1"
```

## What it does

Hover the notch to open it, scroll to change pages. Hover the page dots under the panel and they turn into a row of icons that grow toward the pointer, like a dock; click one to jump to that page. With the cat on, it also turns its head and eyes toward the pointer (full animation only, off in Energy Saver; the *Follow the pointer* switch on the Cat page turns it off). While the pointer nears the notch over another window, however fast it moves, the notch glides down to the cat's two eyes so the window under it (a browser's tabs, say) can be clicked; rest on the eyes for half a second to bring the notch back (the *Shrink to eyes* switch turns this off). The panel stays open for a few seconds after you click in it or close an editor, and every page and sub-view change glides (or is instant when Windows' animation effects are off).

| | |
|---|---|
| **Music** | Cover, controls, progress and synced lyrics in the notch. Scroll over the notch for volume, swipe or middle-click to skip. With the panel open, a sideways two-finger swipe on a touchpad turns the page. |
| **Mixer** | Master and per-app volume sliders. |
| **Clipboard** | Searchable history of text, links, colours, files and images, with pins, and a short preview in the notch whenever you copy something. Password managers' copies are never kept or shown. |
| **Screenshot** | Its own capture: the Screenshot tile freezes the screen and snaps to the window, panel or box under the pointer (or drag your own rectangle), then opens an editor with text, arrows, boxes, pen, marker and pixelate. PrtSc opens this capture too, even where Windows owns the key (Settings → Clipboard → Screenshot turns it off; Alt+PrtSc and Win+PrtSc stay with Windows). The notch also previews Win+Shift+S and Snipping Tool shots, with Copy again, Keep or Delete; Keep never saves a second copy of a file Windows already wrote. |
| **File shelf** | Drop files on the notch, drag them out later. Cards animate in and out. |
| **Search** | `Ctrl+Alt+Space`: apps, files and folders, calculator, Windows settings, web. |
| **Calendar** | Google Calendar with day, week and month views; add and edit events. |
| **Reminders, Notes, Pomodoro** | Quick ones, right in the panel. |
| **Notifications** | Windows toasts and [Claude Code](https://claude.com/claude-code) prompts in the notch. The Notifications page keeps a short history of the toasts Vale caught while it ran (app logo, time, title and body), with pause, per-app exclusion and an optional encrypted copy that survives a restart. It never clears the Windows Action Center. |
| **System** | CPU, memory, disks, the busiest apps and a cleaner that asks before it deletes. |
| **Devices** | Flash drives, external disks and memory cards, with Eject, Open, Detail and Eject all, the reason when Windows won't let go, and a notice when one is plugged in. |
| **Susun** | Arrange your open windows into a layout: pick one (two or three columns, a grid, a main window with a side column, or your own), then click the windows tile by tile, with skip and back. Undo with one click, save an arrangement and re-apply it later. `Ctrl+Alt+S` opens it (off by default, in Settings). |
| **CLI** | A small terminal that starts in your Explorer folder. |
| **Shortcuts** | Tiles for Windows tools, a jump to any panel page, Vale's own Settings, and your own apps, folders and links (added in Settings). |

<table>
  <tr>
    <td width="50%"><img src="assets/screenshots/music-lyrics.png" alt="Music page: cover, the title Senja di Kemang, a progress bar, playback controls and the Lyrics button"><br><b>Music</b> with synced lyrics</td>
    <td width="50%"><img src="assets/screenshots/mixer.png" alt="Mixer page: master volume and sliders for Spotify, Chrome and a muted Teams"><br><b>Mixer</b>: one slider per app</td>
  </tr>
  <tr>
    <td><img src="assets/screenshots/clipboard.png" alt="Clipboard page: a search field and cards for copied text, a link, code, a colour and a screenshot"><br><b>Clipboard</b> history, searchable, with pins</td>
    <td><img src="assets/screenshots/shelf.png" alt="File shelf: six files and folders dropped on the island, one marked missing"><br><b>File shelf</b>: drop now, drag out later</td>
  </tr>
  <tr>
    <td><img src="assets/screenshots/search.png" alt="Quick search: kal typed, the apps Kalender and Kalkulator, and a web search for kal"><br><b>Search</b> on <code>Ctrl+Alt+Space</code></td>
    <td><img src="assets/screenshots/system.png" alt="System page: a CPU ring at 33 percent, RAM and two disks, the busiest apps and a Clean up button"><br><b>System</b> at a glance, with a cleaner</td>
  </tr>
  <tr>
    <td><img src="assets/screenshots/calendar.png" alt="Calendar page in month view with today's events listed on the right"><br><b>Calendar</b> from Google, offline too</td>
    <td><img src="assets/screenshots/shortcuts.png" alt="Shortcuts page: tiles for Screenshot, Clipboard, Search, Emoji, Explorer, Calculator, Task Manager, Windows Settings and Lock screen"><br><b>Shortcuts</b>: Windows tools, page jumps and your own tiles</td>
  </tr>
</table>

<p align="center"><img src="assets/screenshots/notices.png" width="460" alt="Animated notices in the notch: a greeting, a copied item, a screenshot preview, a disk warning and a long lyric line"></p>

Everything is set in **Settings**, from the tray icon, in eight sections: General, Appearance, Cat, Panel, Clipboard, Features, Integrations and About. Appearance holds the island style, accent colour, island background (*Match cat*, Espresso, Graphite, Sea, Night or Plum), notch size and fill; Cat holds the coat, accessory, animation, sleep, the follow-pointer and shrink-to-eyes switches and the greeting. Open mode, page order, shortcuts, hotkeys, lyrics and language are there too. It can also stay behind your windows (experimental). Every option is described in the [guide](docs/guide.md#settings).

## Privacy

Notes, reminders, clipboard history, notification history and the shelf stay on your PC; the clipboard and notification history files are encrypted with Windows DPAPI. Nothing is sent anywhere except to Google Calendar (when you connect it) and, with lyrics on, the artist, title and length of the playing track to [LRCLIB](https://lrclib.net). There is no telemetry. Details in the [guide](docs/guide.md#data-and-privacy).

## Build from source

Rust stable with the MSVC toolchain, and Visual Studio Build Tools with a Windows 11 SDK. From PowerShell:

```powershell
cargo build --release --locked
.\target\release\vale.exe
```

Tests: `cargo test --locked`. More in the [guide](docs/guide.md#build-from-source).

## Credits

Vale is built on the work of others, with thanks:

- **[Mole for Windows](https://github.com/tw93/Mole/tree/windows)** by tw93: the cleaner's list of safe-to-clean folders comes from Mole 1.30.0 (MIT), and the *Automatic · Mole* mode runs your own install of Mole.
- **[DM Sans and DM Mono](https://github.com/googlefonts/dm-fonts)** by Colophon Foundry for Google Fonts (SIL Open Font License 1.1), embedded in the exe.
- **[windows-rs](https://github.com/microsoft/windows-rs)** by Microsoft, with [serde](https://github.com/serde-rs/serde), [serde_json](https://github.com/serde-rs/json) and [chrono](https://github.com/chronotope/chrono) (MIT / Apache-2.0).
- **[LRCLIB](https://lrclib.net)** for synced lyrics, and the **Google Calendar API** for your events.
- **[Claude Code](https://claude.com/claude-code)** by Anthropic, through its hooks.
- Inspired by Apple's Dynamic Island. Vale is an independent project, not affiliated with Apple, Microsoft, Google, Anthropic or Spotify.

Every component with its version and licence: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## License

[MIT](LICENSE) © 2026 hartono0297. Third-party material keeps its own licence; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) and [NOTICE](NOTICE).
