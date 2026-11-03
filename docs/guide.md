# Vale guide

The full reference: every feature, every setting, setup and troubleshooting. For a short overview and the install line, see the [README](../README.md).

**Vale was called Dynamic Island** until version 3.1. Installing 3.1 over it replaces the old install, and Vale moves your data from `%APPDATA%\DynamicIsland` to `%APPDATA%\Vale` on its first start. Version 3.1 also adds a clipboard history, a screenshot preview, a System page with a cleaner, a CLI that follows Explorer, panel pages you can reorder or hide, and one-click Google sign-in.

Design documents (in Indonesian) are in [`docs/design/`](design/).

## Install

Windows 11, 64-bit. Open **PowerShell** (no admin needed) and run:

```powershell
irm https://raw.githubusercontent.com/hartono0297/vale/main/install.ps1 | iex
```

The script downloads the latest [release](https://github.com/hartono0297/vale/releases), checks its SHA-256, installs `vale.exe` (one file, about 2 MB, no runtime needed) to `%LOCALAPPDATA%\Programs\Vale`, adds a Start Menu shortcut and starts the island.

- **Update:** Vale checks GitHub for a newer release once a day (Settings → About → *Check for updates automatically*, on by default) and offers it in the notch with *Update* and *Later* buttons. The offer stays until you pick one (over an open panel it shows as the strip); *Later* hides it and the same version is offered again at the next check (about a day later, or 30 s after the next start). The tray menu (*Check for updates…*, then *Update to X.Y.Z*) and Settings → About → *Check now* do the same on demand. Installing downloads `vale.exe` and its `.sha256` from the release, checks the hash, swaps the exe in place and restarts Vale; if anything fails the old exe stays and the notice links to the release page. Debug builds never check. Running the install line again also updates. Your settings, notes and Google login are kept.
- **Without the script:** download `vale-v<version>.zip` from Releases, extract it, right-click `install.ps1` → *Run with PowerShell*. The exe also runs from any folder.
- **Uninstall:**
  ```powershell
  irm https://raw.githubusercontent.com/hartono0297/vale/main/uninstall.ps1 | iex
  ```
  This removes the island's Claude Code hooks, autostart, shortcut and program folder. Your data stays; run `$env:DI_PURGE = '1'` first to delete it too.
- **SmartScreen:** the exe is not code-signed. If Windows warns about a downloaded exe, choose *More info → Run anyway*.

On first run, Windows asks for access to your notifications: allow it.

## Features

- **Always-visible notch** at the top centre, with a clock when idle. It hides only for fullscreen apps; clicks outside its shape pass through.
- **Hover to open the panel** (or Settings → Panel → *Open mode*: *Hover*, *Hover & stay* until you click elsewhere, or *Click*), scroll the mouse wheel to change pages: **Calendar → Pomodoro → Reminders → Notes → Music → Mixer → Clipboard → Notifications → File shelf → System → Devices → CLI → Shortcuts**. Settings → Panel → *Panel pages* changes the order and hides pages you don't use (a hidden page still opens from its own button or from a Shortcuts tile). The panel **stays open for 4 seconds after you do something in it** (a click, a page turned, a note, event or reminder editor closed), so it no longer snaps shut the moment the pointer is away; a click outside, or a shortcut tile that runs something, closes it at once. Pages slide, the search and sub-views (calendar ranges, the cleaner, the CLI folder menu) fade in, and with Windows' *Animation effects* turned off (Settings → Accessibility → Visual effects) they are instant and the panel opens without its small overshoot.
- **Calendar:** Google Calendar with Month, Week and Day views. The day view is an hour timeline with a line at the current time. Add and edit events in the island's own editor; repeating events, events over several days and events on calendars you can only view open read-only. Events can't be deleted from the island. Works offline from the last sync.
- **Pomodoro:** progress ring, start/pause/reset/skip, a small timer in the notch while it runs. Durations are set in Settings → Features → *Pomodoro*.
- **Reminders:** once, daily, or weekly on one or more days of the week, set in the island's own reminder editor with time shortcuts (*In 30 min*, *In 1 hour*, *Tonight*, *Tomorrow morning*) and a summary of when it rings next. Reminders that came due while the island was off fire at startup, marked "Missed".
- **Notes:** quick local notes, saved when the editor closes.
- **Music:** cover, title, a progress bar and controls for whatever is playing (Spotify, browser, ...), an equalizer in the notch whose five bars follow the sound that is playing (Settings > Appearance > *Equalizer follows the music*; off, or while no device can be read, the bars use a gentle wave), and optional synced lyrics in the notch from [lrclib.net](https://lrclib.net). The lyric notch grows with the line and scrolls a line that is still too long. A button only acts when it is both pressed and released on it, and not in the first half second after the panel opens under the pointer, so music is not paused by accident. Gestures on the island while music plays: the **wheel over the compact notch** (or over the cover on the Music page, unless you were just scrolling through pages) changes the volume, 2 % a notch while a song plays (`"scroll_volume": false` turns it off); the **middle button** plays or pauses, a **swipe** left or right (or a sideways wheel / two-finger touchpad scroll) skips to the next or previous song; `"gestures": false` in `config.json` turns them off. Lines show automatically in time with the song, using the best-synced version lrclib has: the upload whose length matches the playing track comes first, and when a music video only has lyrics timed for the shorter album version, the lines are moved later by the longer intro.
- **Notifications:** every Windows toast shows in the island; click one to open its app. When the notch is idle the toast takes it over for five seconds. While it is in use the toast stays out of the way by default (Settings → Features → Notifications → *Notifications on a busy notch*, *Compact*): over an open panel it is a thin strip at the top with the app's logo, source and title (click it to dismiss); beside the lyrics, the music or the pomodoro it is a small logo on the right, and pointing at the logo shows the full toast for as long as the pointer stays (plus a short grace). A small dot on the island means a toast ended unseen; it clears when you point at the logo or open the Notifications page. *Full* makes every toast cover the notch as before. Turn on Windows *Do Not Disturb* to avoid seeing them twice (Settings → General → *Turn off Windows pop-ups* shows how). A low-battery notice comes at the threshold set in Settings.
- **Notifications page:** a history of the Windows toasts Vale already catches, newest first, one row each: the app's logo (a letter tile in a colour made from its name when Windows gives none), the app name, how long ago (*just now*, *5 min*, the time today, *Yesterday*, then a date, with the year only when it is not this one), an accent dot while it is unread, and `title — body` on one line under it. Move the pointer over a row to light it: **Open** brings the app forward (a click on the row does the same) and marks the row read, **Exclude this app** adds the app to *Excluded apps* and removes all its rows, and the **X** removes only that row from Vale's list. Vale never touches the Windows Action Center: nothing is cleared or dismissed there. The header has *n / max*, a **Pause** switch (the notch still shows toasts, but nothing new is kept) and **Clear all**, which asks for a second click ("Sure?", four seconds) before it empties the list. The wheel scrolls one row a notch while the list is longer than the panel (otherwise it turns pages). To use the keyboard, click the list's background or the title: **Up / Down** move the selection, **Enter** opens it, **Delete** removes it, **Esc** gives the keyboard back; the island never takes the keyboard on its own. Toasts that were already in the Action Center when Vale started are not imported, and an excluded app never reaches the notch or the list. Without notification access the page says so and *Open settings* opens Windows' notification privacy page (Vale never asks for access from the page). With *Save notifications* off a note says so under the header and the entries stay until cleared. Settings → Features → *Notifications* sets *Save notifications*, *Most kept* (10–200, 50 by default, the oldest go first), *Keep after restart* (encrypted with DPAPI, only your Windows account can read it), the *Excluded apps* list (each with *Delete*, and a field with *Add*; case does not matter and a name matches any app whose name contains it) and *Clear notification history* (two clicks). The *Notifications* tile on the Shortcuts page jumps to the page (off by default; add it in Settings → Panel).
- **Claude Code:** "Claude finished" and "Claude needs you" notices through Claude Code hooks; clicking one goes to the session it came from (Claude Desktop, VS Code, Windows Terminal, Orca), never opening anything new.
- **Clipboard:** a history of what you copy (text, links, colours, files and images), searchable, with pins, in the panel. Move the pointer over a card to get its pin, plain-text and delete buttons. Password managers' copies are never kept. The history can be kept across restarts (encrypted with Windows DPAPI) or in memory only, and an optional hotkey (none by default; you record it in Settings → Hotkeys → *Open Clipboard*) opens it. **Copy preview:** every time you copy in another app the notch shows a short preview for 3 seconds: the first two non-empty lines of copied text (or the kind and length when there is one line), the picture's thumbnail with its size, or the file names, with a *History* chip. A click on it opens the Clipboard page (when that page is switched on in *Panel pages*; otherwise it only dismisses it). Nothing shows for a copy a password manager marks as private, for the same thing copied again within 4 seconds, for a copy made from Vale itself, while the Clipboard page is open or a fullscreen app is up, or when *Clipboard history* is off (the preview needs an entry to name). Settings → Clipboard → *Copy preview* (on by default) turns it off; while it is on it replaces *"Copied" for every copy*, which is greyed out until you turn it off.
- **Screenshot (the Screenshot tile):** Vale's own capture. The screen freezes and dims; the box under the pointer lights up and follows it: first the window, then the panel, toolbar or picture inside it (found through Windows UI Automation, a moment after the windows). **Click** takes the highlighted box (or the whole monitor under the pointer when nothing is lit), **drag** draws your own rectangle, **Enter** takes the highlight, **Esc** or a **right click** cancels. The editor opens at once with the *Select* tool ready: **V** Select, **T** Text (click, type; Enter commits, a click elsewhere only finishes the text (the next click places a new one), Shift+Enter adds a line, double-click a text to edit it again), **R** Rectangle, **A** Arrow, **P** Pen, **M** Marker, **B** Pixelate; six colours and three stroke widths; **Ctrl+Z** / **Ctrl+Y** undo and redo; **Delete** removes the selected mark (drag it to move, drag a corner or an arrow's end to resize); **Ctrl+C** copies (PNG and bitmap), **Ctrl+S** saves a new `Screenshot YYYY-MM-DD HHMMSS.png` in your Screenshots folder and never overwrites a file; **Esc** cancels a text, then clears the selection, then closes. Closing with changes (the Close button, Esc, the window's X or Alt+F4) shows *Discard changes?* with **Discard** (red) and **Cancel**: **Enter** discards and closes, **Esc** keeps editing; with no changes the window closes at once. **Copy automatically:** the picture goes to the clipboard (PNG and bitmap) the moment you pick the area, and again about a third of a second after each finished change (a mark added, moved or deleted, a text committed, undo, redo), so Ctrl+V pastes what you see; the clipboard history keeps one card per capture and the notch shows no copy notice for it. It never overwrites something you copied yourself afterwards: it stops for that editor until you press Copy again, and the footer says *Couldn't copy · press Copy* if the clipboard stayed busy. Nothing is written to a file until you press Save, and the frozen screen lives in memory only. Settings → Clipboard → Screenshot → *Copy automatically* turns it off (then nothing is copied until you press Copy; Snipping Tool copies by itself either way). Settings → Clipboard → Screenshot → *Screenshot tool* switches the tile back to Snipping Tool.
- **PrtSc key:** Vale takes over the bare PrtSc key and opens its own capture and editor, whatever *Alat screenshot* says, even when Windows' own "Use the Print screen key to open screen capture" is on (a keyboard hook catches the key, nothing is written to the registry and no sign-out is needed). Alt+PrtSc, Win+PrtSc, Ctrl+PrtSc and Shift+PrtSc stay with Windows, unless you record one of them for a hotkey yourself. Change it or turn it off in Settings → Hotkeys → *Vale screenshot*; the hook is only installed while some hotkey uses PrtSc. If Windows refuses the hook, the row says so in red.
- **Page dots as a dock:** hover the dots under the panel and they become one small icon per shown page, in your page order. The icon under the pointer grows most, its neighbours less; click an icon to open that page. Leaving the dots turns them back.
- **Notch shrinks to the eyes (stay behind off):** when the pointer comes within about 50 dip of the notch and another window lies under it, the notch shrinks to a small pill with only the cat's two eyes (brand colour, also with the cat turned off), so clicks outside the pill reach the window below. Moving away restores it; resting on the pill for half a second, or clicking it, brings the notch back. Over the bare desktop it stays as it is. The eyes blink every 3 to 6 seconds and slide toward the pointer (with *Follow the pointer* on); with Animation not on Full, Energy Saver or Windows animations off they do not blink and jump to the pointer without easing. Settings → Cat → *Shrink to eyes* (on by default) turns it off.
- **The cat follows the pointer:** on the idle view the cat turns its head and eyes sideways toward the pointer across the screen. Only with Animation on Full and Windows Energy Saver off; otherwise it stays centred. Settings → Cat → *Follow the pointer* (on by default) turns it off. With both this and *Shrink to eyes* off, Vale stops watching the pointer altogether.
- **The cat dances to music:** while a song plays, the cat in the music and lyric views nods to the bass (from the same audio the equalizer follows, or a steady beat when that is off) and its note floats up and fades. Only with Animation on Full, Energy Saver off and Windows animations on. Settings → Cat → *Dance to music* (on by default) turns it off.
- **Sideways swipe on the panel:** with the panel open, a sideways two-finger touchpad scroll turns one page per flick, like the vertical wheel (not on Search). On the compact music notch it still skips songs.
- **Search back button:** a back arrow beside the search field does what Esc does.
- **Screenshot preview:** after Win+Shift+S or a Snipping Tool shot the notch shows the shot with *Copy again*, *Keep* and *Delete* (to the Recycle Bin, and the clipboard is emptied). What an ignored preview does is set in Settings → Clipboard → Screenshot (*If ignored*); the default keeps the file.
- **System:** CPU, RAM and disk use on their own page, with one-time warnings when a drive runs low or RAM stays high. The figures glide to each new reading (once a second while the page is open, nothing runs when it is closed).
- **Devices:** the *Devices* page lists the flash drives, external disks (USB or Thunderbolt) and memory cards that are plugged in and have a drive letter, one card each: a name and its letters (`SanDisk Ultra · E:`), what it is and how it is connected (`USB flash drive · exFAT`, `External USB disk · 2 NTFS partitions`, `Card reader · FAT32`), a bar with used / total over all its partitions, and three buttons. **Eject** asks Windows to remove it safely; the button reads *Ejecting…* until Windows answers, and *Eject all* (top right) does every device, one after another. The **folder** button opens Explorer on its first drive and closes the panel. The **i** button unfolds the card in place (one at a time) with the model, the connection (USB 3 or 2 when Windows says, USB otherwise, Card reader, Thunderbolt), the file systems and a line per partition with its size and label; a long list or an unfolded card scrolls with the wheel while the pointer is over it (elsewhere the wheel still turns pages). When Windows refuses, the card says why in a red row (*Can't remove it yet. Still in use: Excel*) with **Try again**: close the app it names, or whatever else has the drive open, and press it. Windows does not always name the program: it may only say another app, a Windows service or a driver is using the drive, and Vale then says just that. Only drives with a letter are listed: internal disks, network drives, optical drives, phones (MTP) and devices with no letter (an unformatted disk, a card slot with no card) are not. Sizes are read when the page opens, and for a drive when it is plugged in; nothing is polled, so an idle island does no work for it. A notice appears when a drive is plugged in (*SanDisk Ultra connected*, with its free space and *Open* to open it in Explorer) and when Vale's Eject says it is *Safe to remove* (not for a drive you pull out yourself); Settings → Features → *Devices* → *Device notifications* turns both off. The *Devices* tile on the Shortcuts page jumps to the page, and an optional hotkey (Settings → Hotkeys → *Open Devices*, none by default) opens it like the Clipboard's and closes it when pressed again.
- **Quick search:** a hotkey (**Ctrl+Alt+Space** by default; Settings → Hotkeys → *Quick search* records another one or turns it off, and never Alt+Space), the tray menu's *Search…* and the *Search* tile on the Shortcuts page all open the island on a search field. It finds apps (from the Start Menu, read once on first use), a few Windows settings pages, files and folders by name (from two typed characters, under Desktop, Documents, Downloads, Pictures, Videos and Music, a few levels deep; the names are read each time it opens, kept in memory only and never sent anywhere, and file contents are not searched), and does arithmetic (`2*(3+4)`, `+ - * / % ^`, brackets; Enter copies the result). `? coffee` searches the web in your browser (Google). Up / Down move, Enter runs (a file or folder opens as a double-click would), Esc or a click elsewhere closes. Icons, a folder's item count and an app's install size (when Windows knows it) fill in a moment after the rows appear.
- **Mixer:** the volume of the default output and of up to four apps playing sound (one row per program, in the order they first appeared while the page is open), plus Windows' system sounds while they play. Press or drag a slider to set a level, click the speaker to mute, scroll over a row for 5 % steps. The page reads the volumes once a second while it shows and nothing otherwise.
- **Stay behind windows (experimental):** Settings → Panel → *Stay behind windows* puts the island below ordinary windows. It comes up when something needs to be seen (a notice, the panel, the search) and when the pointer touches the top edge of the screen over it, and goes back down a moment after the pointer leaves. Off by default. A window covering the top-centre of the screen hides the island until the pointer touches the top edge, and over a maximised window's title bar the island rises over it. Fullscreen apps behave as before.
- **File shelf:** drag files or folders to the notch (a faint margin round it and above it catches the drag, so aim at the top edge of the screen) and the island opens on the shelf with a dashed drop zone; drop them and they stay as cards (up to 12) with their icon, a two-line name and a small X. The panel stays a moment after the drag leaves. Click a card to open the file, drag it out again to use it elsewhere (copy or link, never move), click its X to take it off or *Clear* for all. Cards animate: a new one fades and grows in (files dropped together arrive one after another), one taken off shrinks and fades out where it was, and the rest slide into their new places. With Windows' *Animation effects* off the shelf changes instantly. The shelf keeps paths only, not copies, and only in memory: it is empty after a restart. A card whose file is gone is dimmed.
  **From a phone:** the *HP* pill beside *Clear* opens the phone link. The panel shows a QR code (and the network and address under it); scan it with the phone's camera on the same Wi-Fi and a small page opens in the phone's browser, nothing to install. The phone shows a four-digit code; when it matches the one on the notch, press *Izinkan*. Each new phone asks once per session, and three refusals void the code (press *Buat QR baru* for a fresh one; a code nobody used also runs out after five minutes). Then pick photos, videos or files on the phone: they come one by one into `Downloads\Vale` (written as `.part` first, never over a file of the same name: `IMG_1 (2).png`), with their progress on the notch, and each lands on the shelf as a card tagged *dari HP* that you can drag out like any other. If a batch would not fit in the shelf's 12 cards, one card for the `Downloads\Vale` folder is added instead. *Batal* stops what is coming and deletes what is unfinished; *Selesai* (or *Back* before any phone connected) ends the session and closes the server. The link refuses Public networks (Windows asks once to allow it: choose *Private network*), serves only private addresses; the phone's screen must stay on while it sends (a locked phone's browser stops). Sending files from the shelf to the phone comes later.
- **Shortcuts:** Screenshot, Clipboard, Search, Emoji, Explorer, Calculator, Task Manager, Windows Settings, Lock screen and a tile that jumps straight to a panel page: Calendar, Pomodoro, Reminders, Notes, Music, Mixer, Notifications, Shelf, System, Devices and CLI (Clipboard's tile already opens its page). A jump tile changes the page in place, without closing the panel, and works for a page you have switched off. New installs show the first nine, Shelf and Devices; the other jump tiles are there to add. Vale's Settings open from the sliders button at the top right of every panel page (and from the tray menu); an old config that names `ValeSettings` is read without it. An older config that showed fewer than twelve tiles gets the Devices tile too; one that showed twelve does not (add it in Settings). Settings → Panel → *Shortcuts* has three parts: the tiles that show ("Shown on the page: n/12", up to twelve, four per row) in your order, each with a drag handle, up and down buttons and *Hide* (and *Delete* on your own); *Available* below, grouped as Pages, Windows, Vale and Your own, each with *+ Add* (off while twelve show); and *Restore defaults*, which puts the built-ins back in their default order and on/off and keeps your own tiles after them. *+ Your own shortcut* opens a form in the card: a name (up to 14 characters), the type (App, Folder or Web) and the target; *Browse…* opens the file or folder dialog for an app or folder. The tile is added shown when there is room, off otherwise (the form says why). The same list lives in `config.json`:
  ```json
  "shortcut_tiles": [
    { "tile": "Screenshot", "on": true },
    { "tile": "Lock", "on": true },
    { "tile": "GoShelf", "on": true },
    { "tile": "GoCalendar", "on": false },
    { "tile": "Custom", "label": "Gmail", "kind": "Url", "target": "https://mail.google.com" },
    { "tile": "Custom", "label": "Projects", "kind": "Folder", "target": "D:\\Projects" },
    { "tile": "Custom", "label": "Editor", "kind": "App", "target": "C:\\Tools\\editor.exe" }
  ]
  ```
  `kind` is `App` or `Folder` (an absolute local path such as `C:\\Tools\\editor.exe`; backslashes are doubled in JSON) or `Url` (only `http`, `https`, or `mailto` without a `?`); a label has up to 14 characters, at most eight own tiles, and names are read in any case. Entries that do not pass are left out. The built-in names are `Screenshot`, `Clipboard`, `Search`, `Emoji`, `Explorer`, `Calculator`, `TaskManager`, `WindowsSettings`, `Lock` and `Go` plus a page (`GoCalendar`, `GoPomodoro`, `GoReminders`, `GoNotes`, `GoMedia`, `GoMixer`, `GoNotifications`, `GoShelf`, `GoSystem`, `GoDevices`, `GoCli`). A built-in tile you don't list rejoins right after its neighbour in the default order, shown only if it is on by default (the first nine, `GoShelf`, `GoDevices`) and the page has room. No more than twelve tiles show: Settings will not switch a thirteenth on.
- **Clean up:** from the System page, measure and clean temp files and logs, browser and app caches, developer caches, Autodesk and Adobe caches, old installers in Downloads and old logs beside your own files (both to the Recycle Bin) and Windows Update leftovers (asks for admin through UAC). You tick what goes, and the button asks once more before anything is deleted; only files the scan counted are deleted. Open browsers and files in use are skipped and reported; the island reports the free space it measured afterwards.
- **Clean up with Mole (experimental):** the cleaner's second mode, *Automatic · Mole*, hands the clean to [Mole for Windows](https://github.com/tw93/Mole/tree/windows) if you installed it yourself (see [Mole](#mole) below).
- **CLI:** run one command at a time in PowerShell or cmd on the panel's CLI page. The island takes the keyboard only while you type: click the terminal to start (the panel stays open while you type). Esc hands the keyboard back to the app you were in; a click outside the island releases it to whatever you clicked. History suggestions (Tab, ↑), `cd`, `cls`, Stop / Ctrl+C and an enlarge button. A command keeps running while the panel is closed. The CLI starts in the folder of the last active Explorer window (or in your last folder, Settings → Features → *CLI*), and its folder chip opens a menu of open Explorer windows, favourites and recent folders.
- **The cat, the assistant:** a small cat (a head with ears and plain eyes) beside the clock and on the island's own notices (reminders, pomodoro, Claude Code, battery, cleaner, greeting). While Claude Code works, a CLI command runs or a Mole clean runs, the idle cat turns a little to the left and types on a small laptop; the notch grows about 12 dip on the left for it and shrinks back when the work ends. Its paws tap in turns with full animation (still under Saver, Off, Energy Saver or with Windows animations off), and with *Follow the pointer* on only its eyes glance at the moving pointer, going back to the screen after about a second of stillness. It has faces for idle, happy, focus, listening, singing, alert, typing, asleep, low and waving, and its ears go up when alert and droop when asleep or low; it blinks, falls asleep after a while without activity and wakes when you point at the island. Other apps' notifications keep their own badge, and the open panel shows no cat. Settings → Cat picks its **coat**: *Terracotta* (the default), *Calico* (cream with an orange left ear and a dark right ear), *White* or *Black* (amber eyes and a light lilac rim, so it reads on any dark background), and an **accessory**: none, glasses, a bow, a nightcap, a scarf, a flower or a crown. Accessories show on every face; the bow and the flower sit on an ear and move with it. The app icon and the tray stay terracotta.
- **Greeting by the time of day:** at startup, after a wake or when you unlock the PC, the cat greets you once per part of the day, so a later part of the same day greets again:

  | Hours | Greeting | Says |
  |---|---|---|
  | 04:00–10:59 | Good morning (waving) | today's events and reminders |
  | 11:00–14:59 | Good afternoon (happy) | what is left today |
  | 15:00–17:59 | Good afternoon (waving) | what is left today |
  | 18:00–23:59 | Good evening (happy) | tomorrow's events |
  | 00:00–03:59 | Up late? (sleepy) | that it is late, and the day's events |

  A day with nothing on says so instead of "0 events". Settings → Cat → *Greeting* turns it off; it never shows over a fullscreen app.
- **Studio look:** the Vale palette (terracotta, espresso, cream) with six accent colours, the island's base colour (*Match cat*, Espresso, Graphite, Sea, Night, Plum or Terminal), light/dark windows; DM Sans and DM Mono built into the exe (and JetBrains Mono as an option), with titles and the clocks in DM Sans Semibold; the island's own vector icons. Pill or Notch shape. Indonesian (default) or English.

<p align="center">
  <img src="../assets/screenshots/calendar-day.png" width="420" alt="Calendar day view: an hour timeline with two all-day events on top, a design review and a coordination meeting, and a terracotta line at the current time">
  <img src="../assets/screenshots/notch.png" width="420" alt="Calendar week view in the Notch shape, whose top corners curve into the screen edge; each day is a column of event chips">
</p>

<p align="center">
  <img src="../assets/screenshots/pomodoro.png" width="420" alt="Pomodoro page: a terracotta progress ring around 18:42 left in focus session 2 of 4, with reset, pause and skip buttons">
  <img src="../assets/screenshots/reminders.png" width="420" alt="Reminders page grouped under Today, Tomorrow and a date, with a Missed badge on an overdue reminder and repeat icons on repeating ones">
</p>

<p align="center">
  <img src="../assets/screenshots/notes.png" width="420" alt="Notes page: four note cards with a title, two lines of text and when each was last changed">
  <img src="../assets/screenshots/music.png" width="420" alt="Music page: a sunset cover, the track title, a progress bar at 1:42 of 3:58, playback controls and a Lyrics button">
</p>

<p align="center">
  <img src="../assets/screenshots/clipboard.png" width="420" alt="Clipboard page: a search field and cards for copied text, a link, a colour and a screenshot, one of them pinned">
  <img src="../assets/screenshots/system.png" width="420" alt="System page: CPU, RAM and disk use with a Clean up button">
</p>

<p align="center">
  <img src="../assets/screenshots/screenshot.png" width="420" alt="Screenshot preview in the notch: a thumbnail with its size and Copy again, Keep and Delete">
  <img src="../assets/screenshots/cleaner.png" width="420" alt="Cleaner, Choose yourself mode: categories with the size each frees and a Clean button">
</p>

<p align="center">
  <img src="../assets/screenshots/cli.png" width="560" alt="CLI page inside the panel: git status, ls and git pull with their output, a done 2.1 s chip, the PowerShell shell, and the focused input with the hint that the panel stays open while you type">
</p>

<p align="center">
  <img src="../assets/screenshots/editor.png" width="320" alt="Event editor in the dark window mode: title, date chip with Today, Tomorrow, Thursday and Next week shortcuts, start and end times with the duration, a notes field, Cancel and Save">
  <img src="../assets/screenshots/reminder-editor.png" width="320" alt="Reminder editor: date and time chips, quick times In 30 min, In 1 hour, Tonight and Tomorrow morning, Once / Daily / Weekly with Monday, Wednesday and Friday picked, and the summary Every Mon, Wed, Fri at 09:00">
</p>

<p align="center">
  <img src="../assets/screenshots/robot.png" width="460" alt="The cat's faces: idle, happy, focus, listening, singing, alert, thinking, asleep, low and waving, then a blink and the sizes from 48 to 20 px">
  <br>
  <img src="../assets/screenshots/greeting.png" width="300" alt="Morning greeting notice: the cat waving beside Good morning, 3 events and 2 reminders today">
</p>

## Settings

Open **Settings…** from the tray icon.

<p align="center"><img src="../assets/screenshots/settings.png" width="560" alt="Settings window, Appearance section scrolled down: window mode, clock when idle, the assistant card with Assistant in notch, Animation Full / Saver / Off, Sleep after 10 min and Morning greeting, then Panel pages with an order stepper and a switch per page"></p>

Ten sections, in this order; the window opens on General. Every card has a small title.

The window can be resized (down to 720 × 520) and maximised; the content stays in a centred column and the window opens where and as large as you left it (on screen again if its monitor is gone). While you drag a handle in a list, holding it near the top or bottom edge scrolls the page.

| Section | What's in it |
|---|---|
| General | *Basics*: language (Indonesia / English), start with Windows (also a checked entry in the tray menu). *Alerts*: battery threshold (off on a PC without a battery), disk and RAM warnings, and how to turn off Windows pop-ups |
| Appearance | A live preview of the island. *Island*: style (Pill / Notch), accent colour (Terracotta, Sage, Ochre, Clay, Olive, Plum), **base colour**, **font** (DM Sans, or JetBrains Mono for the notch, panel and Settings; code text stays DM Mono; it switches live), clock when idle and *Equalizer follows the music* (the notch's bars move with the real sound; the island listens to the output device only while they are on screen). The base colour is the island's own colour, for the notch, its notices and the open panel: *Match cat* (the default for a new install) follows the cat's coat (Terracotta → Espresso, Calico → Sea, White → Night, Black → Plum, Espresso with the cat off), or pick Espresso, Graphite, Sea, Night, Plum or Terminal (a flat neutral near-black) yourself; a config that already says Espresso or Graphite keeps it. Every colour keeps its text readable (at least 4.5:1). *Notch*: the closed island's width (160–300 px) and height (28–44 px) on two sliders (the island follows while you drag; 190 × 34 is the default, *Reset* puts it back), and its **fill**, *Solid* or *See-through* with an opacity slider (0–100 %, 30 % to start; at 0 % it stays faintly there so the pointer still finds it, and the text gets a soft shadow so it reads on a light wallpaper). The size applies to the idle, music, lyric and timer views (a lyric notch is never narrower than the width you set); the fill also applies to notices. The open panel always stays solid. *Window*: window mode (Follow Windows / Dark / Light) for Settings and the editors (with the Terminal base colour, a dark window is neutral grey instead of warm brown; a light window is unchanged) |
| Cat | A still preview: the cat large on the island's colour and the idle notch at its real size with the clock, with chips (Idle, Happy, Blink, Asleep, Waving) that change the preview's face only. *Cat*: cat in the notch (on or off), its coat (Terracotta, Calico, White, Black), the base colour in use with *Change* (jumps to Appearance) and the accessory, one tile each showing the cat wearing it. *Motion*: animation (Full / Saver / Off), sleep after 1–60 minutes without activity, *Follow the pointer*, *Dance to music*, *Shrink to eyes* (off, with a note, while *Stay behind windows* is on). *Greeting*: the switch and the five parts of the day with their greeting. With the cat off, its rows fade (except *Shrink to eyes*, which works without it) |
| Panel | *Behavior*: open mode (Hover / Hover & stay / Click), *Stay behind windows* (experimental, off by default). *Panel pages*: the pages the wheel shows come first, in your order, each with a drag handle, up and down buttons and *Hide* (the last page shown cannot be hidden); under *Hidden* are the others, each with *Show*; *Restore defaults* resets the list. *Shortcuts*: the tiles the Shortcuts page shows (up to twelve) in your order, with the same handle, buttons and *Hide* (and *Delete* for your own). *Available*: a *+ Add* for every tile that is off, grouped; *Restore defaults*; and *+ Your own shortcut*, a form for an app, a folder or a web address. Drag a handle to reorder (Esc cancels) or use the up and down buttons |
| Widgets | *On the desktop*: show widgets (hides or shows them all, the state is kept), layer (*On the desktop* behind ordinary windows, or *Above windows*; a pinned note is always on top), *Fullscreen* (*Hide* by default, or *Keep*), lock position, cat peek. *Gallery*: one card per widget with a count and a (+) button; phase 1 has the sticky note (up to 20, each one is also a note on the Notes page). Opens with `--settings widgets` |
| Clipboard | *History*: on or off, how many items, keep after restart, clear when the screen locks, the copy preview (a preview in the notch on every copy; replaces *"Copied" for every copy*, which works only while the preview is off), clear history (asks once more). *Screenshot*: the screenshot tool for the Screenshot tile (Vale or Snipping Tool), *Copy automatically* (Vale's own capture only), the preview in the notch, what an ignored preview does and how long it shows |
| Hotkeys | One card, *In every app*: seven rows, each with a field and its own reset button. Click a field and press the keys you want (Esc cancels, Backspace or Delete turns the row off); it shows the combo as key caps, or *None*. *Quick search* (Ctrl+Alt+Space by default), *Open Clipboard*, *Open Devices* and *Open Arrange* (off by default), *Vale screenshot* (the PrtSc key by default), *Open lyrics* (Ctrl+Alt+L by default) and *New sticky note* (off by default). A combo needs Ctrl, Alt or Win (PrtSc, Pause, Scroll Lock and F13 to F24 may stand alone), may not be one Windows keeps (Alt+Tab, Alt+F4, Win+L, Ctrl+Alt+Del, Ctrl+Shift+Esc, Ctrl+Shift+V and a few more) and may not be another row's; the row says why in red. A row another app already holds says so in red as well, and is tried again when Settings or the tray menu opens. *Reset all* puts the seven back. While you record, Vale's own hotkeys are paused and every key goes to the field, so Win combos and PrtSc can be recorded; the keys are released when the recording ends |
| Features | *Pomodoro*: focus, break and long break length; sessions before a long break (locked while a pomodoro runs). *Music*: lyrics in notch. *Notifications*: how a toast shows on a busy notch (*Compact* or *Full*), save notifications, the most kept, keep after restart, the excluded apps (a list with a field to add) and clearing the history (asks once more). *CLI*: start folder (Explorer / last folder) and shell (PowerShell / cmd). *Devices*: the plug-in and safe-to-remove notifications (on by default). *Cleaner*: protect Cargo & Autodesk caches in Mole |
| Integrations | Google Calendar: status, *Sign in with Google* (or your own client ID / secret under *Advanced*), Disconnect, setup guide. Claude Code: install or remove hooks |
| About | Version, source code, *Updates* (the installed version, *Check now*, or *Update* when a newer one is known), *Check for updates automatically* (once a day from GitHub, on by default), data folder |

**Animation:** *Full* blinks, lifts the cat when you point at it and bounces on alerts. *Saver* drops the blinking and motion but keeps the faces; Full also behaves like Saver while Windows Energy Saver is on. *Off* runs no assistant timers at all: the faces still follow what happens, but the cat does not fall asleep. With the cat off, the island looks and works as without it.

## Mole

The cleaner's *Automatic · Mole* mode runs **Mole for Windows** by tw93, a separate cleaning tool you install yourself. The island never installs, downloads, copies or links Mole: it only starts the `mole.ps1` (in `%LOCALAPPDATA%\Mole\`) or `mo` (on PATH) you already have, as a hidden process, and reads what it prints. Without Mole the mode shows a *How to install* link to Mole's Windows README.

- **Preview first:** the mode always starts with `mo clean --dry-run`, which deletes nothing. Its lines are read into four groups (browser, app, developer caches, temp files and logs); a group the island cannot read says *see log* instead of guessing a number. Mole's own summary line is never used.
- **Confirm:** *Run Mole* turns into *Sure? Delete for good* for 4 seconds; only a second click runs `mo clean`, and only with the options of the finished preview. Any other click, an option change or closing the panel disarms it. What Mole deletes is gone for good (no Recycle Bin).
- **While it runs:** Mole's log shows in the panel (colours stripped, its internal tables left out). The panel may close; the cat types on its laptop in the notch and a notice comes at the end: happy with the free space the island measured itself (before and after, per drive), or low with a *Log* button if Mole failed.
- **Options:** *+ System (admin)* (not ticked) runs `mo clean --system` through a UAC prompt in a separate PowerShell; the island itself never runs as admin. *+ Game media > 90 days* adds `--game-media`. *Protect folders…* opens `mo clean --whitelist`.
- **Protected by default:** before Mole's first preview the island adds the Cargo caches (`.cargo\registry`, `.cargo\git`) and the Autodesk folders under AppData to Mole's whitelist (`%USERPROFILE%\.config\mole\whitelist.txt`), appending only what is missing and never changing your lines. Settings → Features → *Cleaner* → *Protect Cargo & Autodesk caches in Mole* turns this off.
- **Versions:** the island was tested with Mole 1.30.x; another version shows a warning and may still run.

**Licence:** Mole's `windows` branch was relicensed from MIT to **GPL-3.0** on 2026-10-11. Because the island only runs the Mole you installed as its own process, and contains no Mole code, Mole's licence applies to Mole, not to the island.

**Claude Code hooks:** Settings → Integrations → *Install hooks* adds `Stop`, `Notification`, `UserPromptSubmit`, `PostToolUse` and `SessionEnd` hooks to `%USERPROFILE%\.claude\settings.json`. Other hooks are left alone, and a backup is kept as `settings.json.bak-vale`. Also available as `vale.exe --install-hooks` / `--uninstall-hooks`.

## Setup Google Calendar

There are two ways to connect the Calendar page:

- **Sign in with Google** (Settings → Integrations): one click, then pick your account in the browser. This uses Vale's own Google client, which is built into release builds; a build without it (for example one you build yourself) shows only the second way.
- **Your own client ID** (Settings → Integrations → *Advanced: use your own Client ID*): your own Google Cloud project, set up once as below. An own client ID, when saved, is used instead of Vale's. Switching between the two asks you to sign in again.

Setting up your own client:

1. In [Google Cloud Console](https://console.cloud.google.com/), create a new project.
2. **APIs & Services → Library** → **Google Calendar API** → **Enable**.
3. **Google Auth Platform** (OAuth consent screen) → **Get started**: any app name, your email, Audience **External**.
4. **Audience → Test users → Add users**: add the Gmail account whose calendar you want.
5. **Clients → Create client** → type **Desktop app**. Copy the **Client ID** and **Client secret** right away (the secret may be shown only once).
6. In the island: Settings → Integrations → *Advanced: use your own Client ID* → paste both → **Save** → **Connect**.
7. In the browser, pick your account, continue past "Google hasn't verified this app", and **tick both permissions**.

While the project's publishing status is **Testing**, Google expires the login after 7 days and the island asks you to reconnect. To avoid that, choose **Audience → Publish app** (*In production*); no verification is needed for personal use, and the next login lasts.

## Data and privacy

Everything lives in `%APPDATA%\Vale\`:

| File | Contents |
|---|---|
| `config.json` | Settings, including your own Google client ID / secret if you set one (plain text) |
| `notes.json`, `reminders.json` | Notes and reminders |
| `calendar-cache.json` | Last calendar data, for offline use |
| `google-token.bin` | Google login, encrypted with Windows DPAPI (only your Windows account can read it) |
| `cli-history.txt`, `cli-cwd.txt` | CLI history and folder |
| `cli-favorites.txt`, `cli-recent.txt` | The CLI folder menu's favourites and recent folders |
| `clipboard-history.bin` | The clipboard history, encrypted with Windows DPAPI; only when *Keep after restart* is on (thumbnails only, no full-size images) |
| `notification-history.bin` | The notification history (app, time, title and text of each toast, and the app logos), encrypted with Windows DPAPI; only when *Notifications → Keep after restart* is on, and removed when it is turned off |
| `media.log` | Only with `--media-log`: when the island sent a media command and when playback changed (app id and state, never titles), up to 1 MB |
| `lyrics-cache\` | Lyrics already fetched |

- Nothing is sent anywhere except the update check (about once a day, release builds ask GitHub for the latest release; on by default, turn off *Check for updates automatically* in Settings → About), Google Calendar (when connected) and lrclib.net (when lyrics are on: the artist, title and length of the playing track).
- A copy that a password manager marks as private never enters the clipboard history.
- The phone link (file shelf → *HP*) runs only while a phone session is open, only on the local network and only for private addresses: nothing goes over the internet, it is plain HTTP on your Wi-Fi (so a Public network is refused), the phone must be allowed on the notch with a four-digit code, and received files land in `Downloads\Vale`. Closing the view with nothing connected, *Selesai*, ten idle minutes, sleep, a lock or a network change end it.
- Notes and reminders are local only; there is no sync and no telemetry.

## Build from source

1. [rustup](https://rustup.rs) with the default `x86_64-pc-windows-msvc` toolchain.
2. Visual Studio 2022 Build Tools with **Desktop development with C++** and a **Windows 11 SDK**.
3. From **PowerShell or cmd** (not Git Bash, whose `link.exe` shadows the MSVC linker):
   ```powershell
   cargo build --release --locked
   .\target\release\vale.exe
   ```

The C runtime is linked statically (`.cargo/config.toml`), so the exe runs without the Visual C++ Redistributable. Quit the island first if it runs from `target\release`. Tests: `cargo test --locked`.

**Screenshots:** the images above come from the debug build's demo modes (`--demo`, `--demo-robot`, `--demo-cli`, `--demo-editor`, `--settings`). With no island running, `cargo build` and then `.\scripts\capture-screenshots.ps1` regenerates them in English.

**Releasing:** push a `vX.Y.Z` tag. `.github/workflows/release.yml` builds the exe and publishes it with its checksum and a zip.

## Troubleshooting

- **No notifications in the island:** check Windows Settings → Privacy & security → Notifications, then run `& "$env:LOCALAPPDATA\Programs\Vale\vale.exe" --probe-notifications` to print what arrives for 60 seconds.
- **"Sign-in cancelled" / "Google rejected the Client ID" (browser page after the login):** the first means the consent screen was closed or refused: press *Sign in with Google* again. The second means the saved Client ID or secret is wrong: check Settings → Integrations → *Advanced*.
- **"Add a Google client ID first":** this build has no built-in Google client (the status reads "Add a Client ID"): open Settings → Integrations → *Advanced*, paste your client ID and secret and press **Save**.
- **"Protected folder access blocked" (Windows Security) for `vale.exe` in `Pictures\Screenshots`:** Controlled folder access stops programs it does not know from creating or changing files in Pictures. The screenshot preview only reads that folder, but *Save* (when Windows made no file) and *Delete* (move to the Recycle Bin) change it. Allow it once: Windows Security → Virus & threat protection → Ransomware protection → Manage ransomware protection → Controlled folder access → *Allow an app through Controlled folder access* → pick `%LOCALAPPDATA%\Programs\Vale\vale.exe`. A new release replaces the file, so Windows may ask again.
- **"Sign in to Google again":** you switched between *Sign in with Google* and your own client ID; connect once more.
- **"Access blocked" / Error 403 from Google (own client):** your account is not a test user (step 4), or the client is not a *Desktop app*.
- **"Google Calendar API isn't enabled":** enable it (step 2) in the same project as the client, wait a few minutes, connect again.
- **"Google permissions incomplete":** connect again and tick both permissions.
- **"config.json is invalid":** usually a missing or extra comma. The island moves the broken file to `config.json.bak` on its next save; copy your values back from there.
- **Island not visible:** it hides while an app is fullscreen; only one instance can run (check the tray).

## Limits

- Primary monitor only.
- No inline replies or action buttons on notifications; apps that don't use Windows toasts aren't seen.
- Notifications are polled every second (about 2–3% of one core) because the installed exe has no package identity. Builds from source can use `scripts/package.ps1` for event-driven mode.
- Calendar is Google only; events can't be deleted from the island, and repeating events, events over several days and events on read-only calendars can't be edited. New events are single-day.
- Devices lists only drives with a drive letter on a removable device (not phones, network drives or internal disks), and the USB speed is shown only when Windows reports it. Eject needs no administrator rights, but a refused one can only be retried: Vale never closes an app for you.
- CLI commands get no input (stdin), so interactive programs won't work; one command runs at a time.
- The pomodoro timer is not kept when the island quits.

## Fonts and credits

Vale stands on the work of others; the full list, with versions and licences, is in [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md). In short:

- **DM Sans** and **DM Mono** by Colophon Foundry for Google Fonts, and **JetBrains Mono** by JetBrains (ligatures dropped), embedded as Latin subsets, SIL Open Font License 1.1 ([`assets/fonts/OFL.txt`](../assets/fonts/OFL.txt)). [`scripts/make-fonts.ps1`](../scripts/make-fonts.ps1) rebuilds them from pinned upstream sources.
- **Mole for Windows** by tw93 ([github.com/tw93/Mole](https://github.com/tw93/Mole/tree/windows)): the cleaner's list of locations (which cache, temp and download folders are safe to clean, and after how many days) follows version 1.30.0, commit `500dfc4c`, © 2025 tw93, MIT; only that data is used and the island scans and deletes with its own code (full text in [`NOTICE`](../NOTICE)). The *Automatic · Mole* mode runs the Mole you installed (GPL-3.0 since 2026-10-11) as a separate program; no Mole code is copied into or linked with the island (see [Mole](#mole)).
- **Rust crates**: `windows` (Microsoft's Windows API bindings), `serde`, `serde_json`, `chrono` and their dependencies, all MIT / Apache-2.0 or similar.
- **Services**: Google Calendar API, [LRCLIB](https://lrclib.net) for synced lyrics, Google Search for web searches.
- **Works with** [Claude Code](https://claude.com/claude-code) through its hooks.

The notch idea is inspired by Apple's Dynamic Island; Vale is not affiliated with Apple or any other company named here.

Design notes and plans: [`docs/superpowers/`](superpowers/).

## License

[MIT](../LICENSE) © 2026 hartono0297. Bundled and referenced third-party material keeps its own licence, described above and in [`NOTICE`](../NOTICE).
