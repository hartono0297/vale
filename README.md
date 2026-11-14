<p align="center"><img src="assets/logo-256.png" width="96" height="96" alt="Vale logo: a terracotta cat head with two ears and two dark pill eyes"></p>

<h1 align="center">Vale</h1>

<p align="center"><b>A notch for the top of your Windows 11 screen.</b><br>
Music with synced lyrics, calendar, clipboard history, volume mixer, file shelf, quick search and more, one hover away.</p>

<p align="center">
  <a href="https://github.com/hartono0297/vale/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/hartono0297/vale?style=flat-square&color=c8644a"></a>
  <a href="https://github.com/hartono0297/vale/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/hartono0297/vale/total?style=flat-square&color=c8644a"></a>
  <a href="https://github.com/hartono0297/vale/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/hartono0297/vale?style=flat-square&color=c8644a"></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/github/license/hartono0297/vale?style=flat-square&color=c8644a"></a>
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
- **Yours to shape.** Open it on hover, hover-and-stay or click; reorder or hide pages; pick the accent colour, the size of the closed notch (width and height), which monitor it sits on (Settings → Appearance → Notch, or the tray menu; with several screens, Identify numbers them), a solid or see-through fill and the island's background (it follows the mascot, or one of five colours), your own shortcut tiles and hotkeys. Indonesian and English.
- **Private.** Notes, reminders, clipboard, notification history and files stay on your PC. No account, no telemetry.
- **The equalizer follows the music.** The five bars beside the song title move with what is actually playing (bass to treble), not a made-up wave. It listens only while the compact music view is on screen, and one switch in Settings turns it off.
- **A small mascot lives in the notch: a cat, a cockatiel or an octopus.** Its face and ears (the cockatiel's crest, the octopus's arms) tell you what is going on: listening to music, focused on a pomodoro, typing on a little laptop while Claude Code, a CLI command or a clean runs (its eyes glance at the pointer as it moves), asleep after a while with no mouse or keyboard input on the PC (and awake again within a couple of seconds of the next touch). It greets you by the time of day (morning, midday, afternoon, evening, late night), once per period. While a song plays it nods to the beat with a little note floating up. The cockatiel hops when something finishes and the octopus turns red for a moment at a reminder. Now and then, while you are at the PC but not typing, the cockatiel plays a short story around the notch: it pecks at the clock, steals a digit and whistles as if nothing happened. Right-click the notch → *Play the mascot* for one any time, and pick how often in Settings → Mascot → *Automatic mascot animation* (Often, Sometimes, Rarely, Greeting only or Off). Moving the pointer to the notch, a click or a key ends it at once. Stories for the cat and the octopus are coming. Pick the mascot, its coat (cat: Terracotta, Calico, White or Black; cockatiel: Lutino, Grey or White face; octopus: Coral, Red, Purple or Sea blue) and an accessory (glasses, bow, nightcap, scarf, flower, crown, headphones, sailor cap or bow tie).

## Install

Windows 11, 64-bit. Open **PowerShell** (no admin needed) and run:

```powershell
irm https://raw.githubusercontent.com/hartono0297/vale/main/install.ps1 | iex
```

The script downloads the latest [release](https://github.com/hartono0297/vale/releases/latest), checks its SHA-256, installs `vale.exe` to `%LOCALAPPDATA%\Programs\Vale`, adds a Start Menu shortcut and starts it. Vale then sits at the top centre of your screen, with an icon in the tray.

- **First run:** Windows asks for access to your notifications; allow it so toasts show in the notch.
- **Update:** Vale checks GitHub about once a day and offers a new version in the notch with *Update* and *Later* buttons (it stays until you pick one; *Later* brings it back at the next check); the tray menu and Settings → About have *Check for updates*. *Update* downloads the release, checks its SHA-256, swaps the exe and restarts Vale (no GitHub page, no reinstall). Running the install line again also works. Settings, notes and your Google login are kept. After an update the notch says what changed: click *Updated to X.Y.Z* to open the *What's new* card in Settings → About, which lists the latest versions.
- **Without the script:** download `vale-<version>.zip` from [Releases](https://github.com/hartono0297/vale/releases/latest), extract it, right-click `install.ps1` → *Run with PowerShell*. The exe also runs from any folder.
- **SmartScreen:** the exe is not code-signed yet. If Windows warns about it, choose *More info → Run anyway*.
- **Coming from Dynamic Island** (Vale's old name): installing Vale replaces it and moves your data to `%APPDATA%\Vale`.

## Uninstall

Open **PowerShell** and run:

```powershell
irm https://raw.githubusercontent.com/hartono0297/vale/main/uninstall.ps1 | iex
```

This quits Vale and removes its program folder, Start Menu shortcut, autostart entry and its Claude Code hooks. **Your data is kept** in `%APPDATA%\Vale` (settings, notes, reminders, clipboard and notification history, Google login), so a later install picks up where you left off.

To remove your data as well:

```powershell
$env:DI_PURGE = '1'; irm https://raw.githubusercontent.com/hartono0297/vale/main/uninstall.ps1 | iex
```

Offline, the copy of the script installed next to Vale does the same:

```powershell
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Vale\uninstall.ps1"
```

## What it does

Hover the notch to open it, scroll to change pages. Hover the page dots under the panel and they turn into a row of icons that grow toward the pointer, like a dock; click one to jump to that page. With the cat on, it also turns its head and eyes toward the pointer (full animation only, off in Energy Saver; the *Follow the pointer* switch on the Mascot page turns it off). While the pointer nears the notch over another window, however fast it moves, the notch glides down to the cat's two eyes so the window under it (a browser's tabs, say) can be clicked; the eyes blink now and then and glance toward the pointer; rest on the eyes for half a second to bring the notch back (the *Shrink to eyes* switch turns this off). The panel stays open for a few seconds after you click in it or close an editor, and every page and sub-view change glides (or is instant when Windows' animation effects are off). Rest the pointer on an icon button for half a second, in the notch, the panel, a sticky note, Settings or the cards, and a small tooltip in the theme's colours says what the button is, what a click does and its hotkey (the combo you set, if any), in Indonesian or English.

| | |
|---|---|
| **Music** | Cover, controls, progress and synced lyrics in the notch. Scroll over the notch for volume, swipe or middle-click to skip. With the panel open, a sideways two-finger swipe on a touchpad turns the page. When several apps or browser tabs play, the notch prefers the music one over a video or a feed, and Pause and Skip go to the session it shows. With lyrics on, a round button on the Music page (or `Ctrl+Alt+L` by default) opens a floating lyrics card: a small always-on-top window with the synced lines, the cover's colours and the previous, play and next buttons. Click a line to seek (when the player allows it), drag it, resize it from the corner, scroll to look ahead; it hides while a fullscreen app is up and is not reopened at startup. |
| **Mixer** | Master and per-app volume sliders. |
| **Clipboard** | Searchable history of text, links, colours, files and images, with pins, and a short preview in the notch whenever you copy something. Password managers' copies are never kept or shown. A *Clear all* button in the page's title row empties the history (asks once more; pins stay). Drag an image card out of the page to drop it into WhatsApp, a website's upload box or a folder: Vale hands over a PNG file. |
| **Screenshot** | Its own capture: the Screenshot tile freezes the screen and snaps to the window, panel or box under the pointer (or drag your own rectangle), then opens an editor with text, lines, arrows, boxes, pen, marker and pixelate (Settings → Clipboard → *After capture* can keep the frozen screen up instead, *On screen*: you draw right on the picked region with a tool strip beside it, then Copy, Save or Open in editor; nothing is copied before that). The picture is already on the clipboard when the editor opens and follows your edits, so Ctrl+V works at once (Settings → Clipboard → *Copy automatically* turns this off; it stops by itself if you copy something else). PrtSc opens this capture too, even where Windows owns the key (Settings → Hotkeys changes or clears the key; Alt+PrtSc and Win+PrtSc stay with Windows unless you assign them). The notch also previews Win+Shift+S and Snipping Tool shots, with Copy again, Keep or Delete; Keep never saves a second copy of a file Windows already wrote. After any Save (the notch, the on-screen bar or the editor) the notch says where it went, and clicking that notice opens the folder in File Explorer with the file selected. |
| **File shelf** | Drop files on the notch, drag them out later. Cards animate in and out. Image cards show a small picture, and right-click one for *Preview image* to see it larger in the panel. The *HP* button opens a QR code: scan it with a phone on the same Wi-Fi, or anywhere through [Tailscale](https://tailscale.com) (the *Wi-Fi | Tailscale* switch at the top of the QR view; Tailscale must be installed and signed in on both the PC and the phone), and the phone can send files (they land in Downloads\Vale and on the shelf) and download every file on the shelf from its *From laptop* tab, one by one or all at once. No approval step and nothing to tick; a card taken off the shelf stops being shared at once. |
| **Search** | `Ctrl+Alt+Space` by default (change it in Settings → Hotkeys): apps, files and folders, calculator, Windows settings, web. |
| **Calendar** | Google Calendar with day, week and month views; add and edit events. |
| **Reminders, Notes, Pomodoro** | Quick ones, right in the panel. |
| **Record** | Record a meeting: the mic, the PC's audio or both, to an `.m4a` (your voice and the others on separate channels). `Ctrl+Alt+R` starts and stops (change it in Settings → Hotkeys), or use the Record page or the tray; pause and resume on the page. The Record page keeps the recorder on the left (the record button, a switch each for the mic and the PC audio with the device under it, live captions) and the tab's recordings on the right, grouped by day, with one Transcribe / View button per row and a line that opens the folder when more recordings are there than fit. Mute the mic or the PC audio right on the notch (click its level bars), on the page, from the tray, or with `Ctrl+Alt+M` (mic, only while recording); pick which mic and which speaker or headset to record (or follow Windows), also mid-recording. Echo cancellation keeps what your speakers play out of your mic track (on by default, Settings → Features → Recording). The notch shows a red REC with the time and two level bars, the tray icon gets a red dot, and a notice opens the file when it is saved. Recordings go to Documents\Vale\Rekaman\Audio and screen recordings to Documents\Vale\Rekaman\Video: one folder, a subfolder for each kind. If Windows' ransomware protection guards Documents, that folder is your user folder's Vale\Rekaman instead. Change it in Settings → Features → Recording; the recordings you already have move to the new subfolders once, when you update. **Transcript:** turn it on in Settings → Features → Transcripts and Vale writes the meeting down on this PC, offline, saying who spoke (you or the others) and when. It uses Whisper large-v3-turbo if you have a GPU and Whisper small if not; you download the engine and the model once from that card. It starts by itself after each recording (you can switch that off and press Transcribe on the saved notice or the Record page instead), and any recording can be transcribed again with the other model or in another language. The transcript window lets you click a line to hear that moment, copy the text, copy it with a ready-made prompt to get a summary from an AI chat, save it as a note, or open the `.md` or `.srt` saved next to the recording. |
| **Screen recording** | Record a tutorial to an `.mp4`. `Ctrl+Alt+Shift+R` (or the Screen tab of the Record page, or the tray) opens a picker: click a whole screen, a window (the video follows it when you move it) or drag an area, choose whether the mic and the PC's audio go in, then Record. The notch counts 3, 2, 1 (Esc cancels) and then shows REC with the time, "Screen" and the mic's level bars; click them to mute the mic. Pause, stop and discard are on the Screen tab. Vale's own windows (the notch, the panel, Settings, sticky notes, menus) never appear in the video; the cursor does. Your voice and the PC's audio are mixed into one track you hear in both ears. Minimising the recorded window pauses the video; closing it saves what was recorded. A notice with a thumbnail opens the file when it is saved. Videos go to the Video subfolder of the recordings folder (see Record); Settings → Features → Screen recording sets the video quality (Original, 1440p, 1080p or 720p; a smaller window or area is never enlarged, and the row shows roughly how many MB a minute takes on your screen) and 30 or 60 frames per second; the Quality row on the Screen tab of the Record page sets both too. With transcripts on, the video gets its `.srt` next to it, so VLC and Film & TV show it as subtitles. One recording at a time: audio and screen recording don't run together. |
| **Zoom while recording** | During a screen recording, `Ctrl+Alt+Z` zooms the video in on your cursor and the camera follows it smoothly; press it again to zoom out. Your own screen never changes, only the video. While zoomed, `Ctrl+Alt` + mouse wheel sets the zoom from 1.5× to 4×, and the notch shows the level. The video also gets a soft yellow highlight under the cursor and a ripple on every click; turn either off in Settings → Features → Screen recording. The shortcut is only taken while you record the screen. |
| **Live captions** | Read what is being said while it is said, in a meeting or a video without subtitles. `Ctrl+Alt+C` (or the Live captions switch on the Record page, or the tray) opens a caption card with the last few lines; the sentence still being spoken shows in grey and turns white when it ends, a few seconds later. It works with or without a recording, runs on this PC with the Transcripts engine and model (so Transcripts must be on), and listens to the PC's audio; turn on "Mic too" in Settings → Features → Live captions to add your own voice, labelled Me and Others. The card stays on top, even over a fullscreen video, and never takes the keyboard away from the app you are in. Drag it, resize it, Ctrl + mouse wheel changes the text size, the mouse wheel scrolls back, ID / EN / Auto switches the language, Copy puts the lines on the clipboard. Captions are never saved to a file: turn them off and the lines are gone (the recording's transcript is the saved text). On battery they pause until the charger is back, unless you press Keep going or turn on "Keep running on battery". "Captions in the notch" also shows the last sentence in the notch. Vale's screen recordings never show the card. Off by default; nothing runs while it is off. |
| **Sticky notes** | Put any note on the desktop as a little paper: text plus tick boxes (type `- ` to start a list, click a box to tick it), five pastel papers or a dark one, three text sizes. Drag it by its whole top strip, or hold Alt and drag from anywhere (it snaps to a grid and to other notes), resize it from the corner, double-click the strip to roll it up, pin it above your windows. *Remind me* (15 min, 1 hour, 17:00, tomorrow 09:00 or your own time) brings the note to the front with a short wiggle and a notice in the island with *Snooze 10 min* and *Open*; reminders missed while the PC was off fire when it wakes. Make it yours with a handwriting or mono font, ruled, grid or dotted paper, washi tape or a push pin, a round stamp (Important, Done, Urgent, Like, Later, Idea), a see-through level and an optional slight tilt (Settings → Widgets). A checklist shows its progress and bursts paper confetti when the last box is ticked. Start from a template (Shopping, Today, Meeting, Idea, Weekly) with a right-click on the (+). Drag text, a link or a file from another app onto a note to add it as new lines (a file adds its full path), or turn on *Fill from clipboard* (Settings → Widgets) so a note from the hotkey or the tray starts with the copied text. Hold Shift and drag a note onto another to make a stack: ‹ › or the mouse wheel over the strip flip the sheets, and *Take out of stack* in the right-click menu puts one back beside it. Press a note against the left or right edge of the screen (or choose *Tuck to edge*) and it becomes a small tab, and the note grows out of that tab (a quick scale and fade) when you point at it. *Send to phone* shows the note, read only, on the phone link. While you type in a note, a thin dark **format bar** appears at its bottom: bold, italic, underline, strikethrough, bullet list, checklist, a highlighter (four colours) and text colour (red, blue, green), and *Add image* (Ctrl+B / I / U, Ctrl+Shift+X, Ctrl+Shift+8, Ctrl+Shift+9, Ctrl+Shift+H). Up to four pictures sit above the text: pick a file, paste a screenshot with Ctrl+V or drag image files onto the note; click one to open it, or the ✕ to remove it. Copying between notes keeps the formatting, text pasted from elsewhere comes in plain, and the phone page shows the formatting. The cat peeks over the note you touched last, or over an overdue one: it holds a clock, smiles when a list is done, and sleeps on a note untouched for three days. It is the same note as on the Notes page and remembers its place per monitor. Make one from Settings → Widgets, the tray menu, the sticky-paper icon on a note card (or right-click the card: *Stick to desktop* / *Unstick from desktop*), the (+) in a note, or a hotkey you set. A new note opens on top of your windows, so you see it at once, and goes back to the desktop as soon as you click another window. Up to 20. Lose track of one? Open the **sticky note list** (the ≡ button in a note's header, *Sticky note list* in the tray menu, or a hotkey you set): every note is a row with its title, the start of its text, when it changed and little marks for pinned, rolled up, tucked, in a stack, reminder and checklist progress. Type to search the titles and the text, press Up and Down and Enter (or click a row) and that note comes out of its tuck, unrolls, jumps to the top of its stack, comes to the front and takes the keyboard. Delete a row (the Delete key on a row you picked with the arrows, or right-click) and *Undo* stays at the bottom for 5 seconds. |
| **Notifications** | Windows toasts and [Claude Code](https://claude.com/claude-code) prompts in the notch; a toast shows which app sent it (its logo and name, plus the site for a browser's web notification when Windows passes it on). The Notifications page keeps a short history of the toasts Vale caught while it ran (app logo, time, title and body), with pause, per-app exclusion and an optional encrypted copy that survives a restart. Clicking a toast (or Open on the page) goes to where it came from: the browser tab that raised it, or the app's own window, even one hidden in the tray, never a new browser window. Clicking a Claude Code notice ("Claude finished", "Claude needs you") goes to the session it came from: its Claude Desktop Code-tab session, VS Code (and the extension's Claude tab), or the Windows Terminal / Orca window and tab, without launching anything new. It never clears the Windows Action Center. While the notch is in use, a toast is compact by default: a thin strip at the top of an open panel, or a small logo beside the lyrics, the music or the pomodoro (hover the logo to read it, an unread dot waits otherwise); *Full* in Settings brings back the full toast. A timed notice shown in full has a small ✕ at its right end while the pointer is over it: it drops the notice without opening anything, so a toast over a fullscreen video can be closed without leaving the video (it stays in the history). |
| **System** | CPU, memory, disks, the busiest apps and a cleaner that asks before it deletes. |
| **Devices** | Flash drives, external disks and memory cards, with Eject, Open, Detail and Eject all, the reason when Windows won't let go, and a notice when one is plugged in. |
| **Susun** | Arrange your open windows into a layout: pick one (two or three columns, a grid, a main window with a side column, or your own), then click the windows tile by tile, with skip and back. Undo with one click, save an arrangement and re-apply it later. Give it a hotkey in Settings → Hotkeys (none by default). |
| **CLI** | A small terminal that starts in your Explorer folder. |
| **Shortcuts** | Tiles for Windows tools, a jump to any panel page, and your own apps, folders and links (added in Settings). |

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

Everything is set in **Settings**, from the tray icon, in ten sections: General, Appearance, Mascot, Panel, Widgets, Clipboard, Hotkeys, Features, Integrations and About. Appearance holds the island style, accent colour, island background (*Match mascot*, Espresso, Graphite, Sea, Night, Plum or Terminal; Terminal also makes the dark Settings and editor windows neutral grey instead of warm brown), font (DM Sans or JetBrains Mono, switches live), notch size and fill; Mascot holds the choice of cat, cockatiel or octopus, its coat, the accessory, animation, sleep, the follow-pointer, dance-to-music and shrink-to-eyes switches, the greeting and how often the mascot plays its stories. Widgets holds the desktop notes' switches (show, layer, fullscreen, lock, cat peek) and a gallery with a (+) per widget; Hotkeys holds all eight global hotkeys (quick search, Clipboard, Devices, Susun, the Vale screenshot, the lyrics card, a new sticky note and the sticky note list, which have no default combo): click a field and press the keys you want, with a reset for each and for all. Settings also opens from the sliders button at the top right of every panel page. Open mode, page order, shortcuts, lyrics and language are there too. It can also stay behind your windows (experimental). The Settings window can be resized or maximised and opens where you left it. To move your setup to another PC, use Settings → About → *Export…*, then *Import…* on the other PC: the file holds only general settings (look, mascot, hotkeys, pages, shortcuts and the like), never notes, histories, your Google login or client, excluded apps, your own shortcut tiles, folders or devices, and Vale keeps a backup of the old settings and restarts. Every option is described in the [guide](docs/guide.md#settings).

## Privacy

Notes, reminders, recordings, transcripts, clipboard history, notification history and the shelf stay on your PC; a recording's audio and text never leave it, live captions are made on this PC and never written to disk, and the transcript engine and model are downloaded only when you click Download in Settings; the clipboard and notification history files are encrypted with Windows DPAPI. An image you drag out of the Clipboard page is written as a PNG to your temp folder (`%TEMP%\Vale\drag`) for the drop, and removed 10 minutes later, when Vale quits, when you delete its card or, with *Clear when the screen locks* on, when the screen locks. Nothing is sent anywhere except the daily update check to GitHub (on by default, off in Settings → About), Google Calendar (when you connect it) and, with lyrics on, the artist, title and length of the playing track to [LRCLIB](https://lrclib.net). There is no telemetry. Details in the [guide](docs/guide.md#data-and-privacy). The phone link of the file shelf runs only while a phone session is open, and the files land in `Downloads\Vale`. It has two modes, picked with the switch on the QR view. **Wi-Fi** stays on the local network: nothing goes over the internet, and it is plain HTTP on your Wi-Fi. **Tailscale** goes through your own Tailscale network, which you install and sign in to yourself on the PC and the phone: WireGuard encrypts it from the phone to the PC, and it may pass through Tailscale's relay servers (still encrypted) when the two cannot reach each other directly. Vale itself sends nothing to Tailscale; it only listens on the address Tailscale gave the PC, and only devices in your tailnet can reach that address. Anyone who scans the QR while it's on screen (on the same Wi-Fi, or in your tailnet, including devices other people shared into it) can send files and download the whole shelf (the QR renews itself every five minutes while the QR view shows; a phone that already scanned stays in until the session ends). Nothing else on the PC is visible to it: no folders, no paths, and a file stops being available the moment its card leaves the shelf. In Wi-Fi mode, Public networks are allowed, with a warning on the notch (avoid cafés and hotels). **Firewall:** if the phone cannot connect (on a Public Wi-Fi, or over Tailscale), allow Vale in Windows Firewall for that network's category; Vale changes no firewall settings.

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
- **[DM Sans and DM Mono](https://github.com/googlefonts/dm-fonts)** by Colophon Foundry for Google Fonts (SIL Open Font License 1.1), embedded in the exe, and **[JetBrains Mono](https://github.com/JetBrains/JetBrainsMono)** by JetBrains (SIL Open Font License 1.1), the optional UI font.
- **[windows-rs](https://github.com/microsoft/windows-rs)** by Microsoft, with [serde](https://github.com/serde-rs/serde), [serde_json](https://github.com/serde-rs/json) and [chrono](https://github.com/chronotope/chrono) (MIT / Apache-2.0).
- **[LRCLIB](https://lrclib.net)** for synced lyrics, and the **Google Calendar API** for your events.
- **[Claude Code](https://claude.com/claude-code)** by Anthropic, through its hooks.
- Inspired by Apple's Dynamic Island. Vale is an independent project, not affiliated with Apple, Microsoft, Google, Anthropic or Spotify.

Every component with its version and licence: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## License

[MIT](LICENSE) © 2026 hartono0297. Third-party material keeps its own licence; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) and [NOTICE](NOTICE).
