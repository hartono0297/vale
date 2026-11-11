---
tags: [memory, index]
---

# Project memory

One note per decision, gotcha or convention. Link each new note here.

## Overview

- [[v3-studio-overview]]: the v3 Studio redesign, one line per decision

## Decisions

- [[studio-palette-accent-aliases]]
- [[studio-font-sources]]
- [[exe-size]]: size measured per change (the screen recording zoom, 2026-11-11, +8.5 KiB to 4,469,760 bytes, is the last entry); icon small sizes as PNG (-38 KB), opt-level z option (-326 KB) left to the owner
- [[studio-icons]]
- [[studio-calendar-day-view]]
- [[studio-media-progress-timer]]
- [[studio-editor-frame]]
- [[studio-reminder-editor]]
- [[bata-robot-expressions]]
- [[bata-idle-power-budget]]: no continuous timer or thread at idle (one-shots, frame clock only while something moves); since 2026-11-06 the closed island redraws and presents only a dirty rect (`ui/dirty.rs`, `UpdateLayeredWindowIndirect`, aliased pixel-aligned clip, Full on morphs/panel/any signature change), drawn on a persistent software WIC bitmap target the size of the surface (rebuilt with it) and copied row by row into the DIB, no `BindDC` and a `TIMER_EQ` tick that would repeat the last frame is skipped (`EqKey`, `draw_gen`), `DI_DIRTY_CHECK=1` self-check
- [[notch-part-redraw-not-merged]]: round 3 (several small dirty rects in the music views, text layout cache) is on branch `task/notch-parts`, not merged: no clear CPU gain in the lyric view (2026-11-07); measure the compact view before reviving it
- [[bata-morning-greeting]]
- [[greeting-periods]]: the cat greets once per period (5) per day by local time, not just mornings
- [[lyric-notch-width]]
- [[compact-padding]]: one 16 dip inset (COMPACT_INSET) for the compact views; shapes grow (media +12, pomodoro +22, lyric +10 with the robot), text room unchanged
- [[notch-size-and-fill]]: Tampilan -> Notch: width/height sliders (160-300 x 28-44), solid or see-through background, the panel stays solid
- [[notch-monitor]]: which screen the notch shows on: `notch_monitor` (Primary or a device path), fallback to the primary and back, hot-plug on `DBT_DEVNODES_CHANGED` (TIMER_DISPLAYS 41), Settings map + radio list, tray submenu, the Identify plates, `DI_MONITORS` / `DI_NOTCH_MONITOR`, hidden over a fullscreen window on its own monitor even when focus is elsewhere (z-order walk, desktop exception)
- [[settings-appearance-scroll]]
- [[media-click-guards]]
- [[media-diagnosis-log]]
- [[media-session-priority]]: every media session is ranked and the best is shown (playing, music, Windows' current, the one shown before); music = known player or artist + album; per-session subscriptions, timeline and thumbnail only for the shown one; commands go to the shown session by key
- [[cli-explorer-folder]]
- [[system-page]]
- [[cleaner]]
- [[mole-mode]]
- [[clipboard-history]]: since 2026-11-06 only the newest image keeps its original raw; older originals are PNG-compressed in memory by a worker (`clipboard/pack.rs`), FULL_BUDGET 16 MB counted in compressed bytes, unpacked on paste
- [[notification-history]]: the Notifikasi page (13th, after Clipboard): a capped, optionally encrypted history of the toasts, exclusion rule, TIMER_NOTIF_SAVE (34), keyboard only by click, logo path, notch toast logo + app eyebrow, browser site (file v2) (v3.9); since 2026-11-06 the unpackaged Poll mode reads every 1 s while active, every 5 s after 60 s without input and not at all while locked or the display is off (`poll_due`, `PollGate`), and the first read after a pause is quiet (history only)
- [[toast-while-busy]]: "Notifikasi saat notch dipakai" (Ringkas default, Penuh): a real toast over a busy notch is a strip in the open panel or a 20 dip logo slot beside a compact view, full when idle (and then latched), the newest replaces it, peek on the logo, unread dot, strip motion, no new timer
- [[notice-close]]: the ✕ on a timed full notice (not Screenshot, ClipPreview, Arranged or the sticky kinds): closes it with no effect and no unread dot, queue advances, Hidden in fullscreen, `hover_blocked` + HoverTracking quiet flag, 24 dip hit square and a permanent 18 dip text reservation, 120 ms fade on the FrameClock, contrast numbers, `--demo-notice-close`
- [[notification-open]]: opening a caught toast: one routine for the notch click and the page's Open, browser tab via UIA or the app's own window (tray-hidden too), never a new browser window or profile picker, `--probe-open`
- [[claude-notice-open]]: clicking a "Claude selesai" / "Claude butuh respon" notice goes to its session: `--hook` captures the host window (process chain, skip list, `cli_direct`, console title), side registry, four rules (Desktop sidebar row, VS Code link, terminal tab, window only), never launches, `--probe-claude-open`
- [[clipboard-notice]]: a 3 s preview in the notch on every external copy (text lines, thumbnail, files), skip rules, supersedes "Disalin untuk semua salinan", no timer
- [[screenshot-preview]]: preview matching of the Windows file; Simpan never writes a duplicate (v3.6); the "Disimpan · folder · klik untuk buka" notice carries the file path and a click reveals it in Explorer (2026-11-04)
- [[screenshot-snap]]: own region capture that snaps to windows and boxes, the editor after it, UIA budget, own-copy registry, auto-copy ("Salin otomatis": worker + debounce + guard + flush on close), PrtSc key opens Vale's capture (low-level keyboard hook on its own thread, now a hotkey row, nothing in the registry), and since 2026-11-02 "Setelah capture" = Langsung di layar: inline drawing on the frozen screen (Lightshot style; strip and bar placement, selection handles, frame-px annotations, managed own-copy on Copy/Save, shared text and icons modules, Line tool, `--demo-snap inline`), and since 2026-11-04 the saved notice is clickable (`Effect::RevealFile`, `sources/reveal.rs` worker, foreground rule, saved sink for the inline capture and the editor, failed inline save says "Gagal menyimpan")
- [[panel-pages]]
- [[vale-rename]]: the Vale rename, what keeps the old name, and the 2026-11-01 repo rename to hartono0297/vale
- [[google-built-in-client]]
- [[github-pages]]: site/ goes to GitHub Pages through the Actions source (Pages cannot serve site/ directly); Geist woff2 fonts, one preload; site/img holds only logo.svg
- [[site-landing]]: the landing page, "Night mix" (2026-11-03): a sky that mixes dusk, night and dawn by page progress (CSS on all four pages, a star canvas on index), the interactive hero cat (coats, greeting, poke, purr, drag onto the notch, the wish constellation), a marquee, the pinned story that grows the panel and flips through the 14 pages, six live playground cards, the page-wide copy toast and the greet state in the live notch, the walking cat and the sleeping-cat easter egg, the go-to-top cat tail (flush at the right edge, two-ring outline for contrast, CSS-only swish, yank on activate), one animation loop, fixed two-pill header with the measured pill clearance rule, tokens and the screenshot-based contrast sweep, the shared markup contract, no-script and reduced motion, `scripts/check-site.py`, the command boxes (joined Copy control, phone icon button, `is-fade`), the icon-only Menu pill and phone spacing, tap targets, what was verified
- [[v3-1-handoff]]: where v3.1 stands, how to continue on another PC
- [[open-mode]]: Mode buka (Hover, Hover & tetap, Klik) and notices with a button
- [[panel-linger-and-motion]]: the panel stays open 4 s after an action; every page and sub-view change moves (slide, fade, reveal), reduced motion, budget
- [[scroll-volume]]: the wheel over the music notch / cover turns the master volume 2 % a notch
- [[lyrics-sync]]: position rate, resync tolerance, lookahead, no manual offset since 3.9, lrclib title clean + search fallback
- [[shortcut-tiles]]: Pintasan tiles chosen in Settings (page jumps, own tiles added in a form; the Vale Settings tile retired in v3.12), drag to reorder
- [[gestures]]: middle click, swipe and sideways wheel on the music views; sideways swipe turns panel pages (v3.6)
- [[v3-2-handoff]]: where v3.2 stands, what is unverified, owner steps
- [[v3-3-handoff]]: v3.3 (icon refresh, stay-behind rise, shelf drop, file search), owner checks, minors left
- [[v3-5-handoff]]: v3.5 (editor close bar, Devices, notch size and fill, shelf motion, clipboard notice), what was cancelled, owner checks
- [[readme-structure]]: README with gallery, install and uninstall, credits in THIRD-PARTY-NOTICES.md; long guide in docs/guide.md; APNG tours
- [[stay-behind]]: experimental setting, island below ordinary windows, rises at the top edge with a short fade and slide (v3.3)
- [[shelf]]: file shelf page, OLE drop target, drag out (paths only, in memory); a drop latches the panel (v3.3)
- [[shelf-phone]]: the HP pill, a QR link and a small local HTTP server to receive files from a phone and send it the shelf: private LAN only, `Downloads\Vale`, session ends, power, ids, Range, SVG never inline, WIC thumbnails, gotchas, what is not verified live (2026-11-04). **v3.19 simplified it:** scan the QR and you are in (no approval, no code, no ticking), the token renews itself every 5 minutes, the whole shelf is shared (`phone_sync_shared` + a loop guard), Public networks are allowed with a warning, the Firewall gotcha is docs only. **Run B** also serves a sticky note as a read-only page (`GET /note`, `phone/note_page.rs`, "Kirim ke HP"; the island side is in [[widgets]])
- [[shelf-motion]]: shelf cards animate in (200 ms), out as ghosts (180 ms) and across (220 ms), no timer, per-draw alpha, reduced motion (v3.5)
- [[search]]: quick search on a hotkey: apps, calculator, Windows settings, web; back button returns to the previous page
- [[search-files]]: files and folders by name in six user folders, own walker with caps, icons/sizes/counts from a background worker
- [[mixer]]: Mixer page, per-app volume through a Core Audio worker thread
- [[devices]]: Devices page (Perangkat), safe eject of flash drives, external disks and cards through a worker thread, no polling
- [[notch-peek-eyes]]: with stay-behind off, the notch shrinks to the cat's two eyes while the pointer is near it over a window, comes back on a 500 ms rest or a click; a fast entry (also caught on the island's WM_MOUSEMOVE) shrinks too; 320 ms ease-in-out eyes morph; shares TIMER_POINTER (33); since v3.11 the eyes look toward the pointer ("Ikuti kursor") and blink every 3-6 s on TIMER_BLINK
- [[cat-looks-at-pointer]]: the idle cat turns head, ears and eyes toward the pointer sideways, eased on the frame clock, clamp checked by a test; "Ikuti kursor" turns it off (v3.8)
- [[cat-colours-and-surfaces]]: four coats (Terracotta, Calico, White, Black), five island surfaces plus "follow the cat", contrast numbers, lenient config (v3.8)
- [[terminal-windows]]: Terminal surface + dark window gives a neutral dark window palette (#1B1D1F family) for Settings, the editors, popovers and the screenshot editor; `window_for`, the three caches fixed (editor brushes, Settings title bar, shot editor `restyle_open`), live switch, demo env vars
- [[ui-font-option]]: Settings -> Tampilan -> Font (DM Sans or JetBrains Mono): config key `font`, lenient read, live switch path, editors stay Sans, `DI_FONT`
- [[cat-accessories]]: seven accessories, the one-silhouette paint order (rim, ears and head, calico patches cut to the whole silhouette, marks, accessories), the rejected mockups, the clamp test (v3.8)
- [[eq-follow-audio]]: the compact view's equalizer bars follow the real music (WASAPI loopback, five bands, per-band auto-gain, `eq-capture` thread only while `TIMER_EQ` runs and "Equalizer ikut irama" is on, sine fallback)
- [[cat-dances-to-music]]: the music and lyric cats nod to the bass (600 ms fallback beat) with a floating note, "Joget ikut musik", `dance_on` gate, note clamp under short notches, TIMER_EQ now also in the lyric view while dancing (v3.10)
- [[cat-types-while-working]]: work running (Claude, CLI, Mole) gives the idle cat a laptop and typing paws instead of Think, eyes-only glance at the pointer (1.2 s rest), island grown 12 dip on the left (`idle_shape_typing`, `idle_layout_lead`), `TIMER_TYPING` (40) one-shot, gate, demo scenes (2026-11-01)
- [[arrange]]: Susun (v3.10), the window layouts: tree model with built-ins, gap maths, pick step with a click-through-gotcha overlay, mover on a worker with frame compensation and undo, saved arrangements, hotkey id 4, Page::ALL 14 / Shortcut::ALL 22, own pick overlay looks fullscreen to Windows (check skipped while it is up)
- [[lyrics-card]]: the floating "open lyrics" card (2026-11-04): own always-on-top layered window with synced lyrics on an art-coloured gradient, `src/lyrics_card/` (layout, colour, window), `app_lyrics_card.rs`, opened by the Musik page's round button or Ctrl+Alt+L (hotkey row 6), click a line to seek (`MediaCommand::Seek`, `MediaInfo::can_seek`), wheel scroll with a 3 s return, drag and resize saved in `config.lyrics_card`, contrast numbers, timers and power, fullscreen hide, `--demo-lyrics-card [scene]`
- [[hotkeys]]: the global hotkeys (row 14 live captions `Ctrl+Alt+C`, always registered, 2026-11-11; row 11 zoom, only while recording the screen, 2026-11-11); the eight global hotkeys as recordable combos (`config.hotkeys`, text form "Ctrl+Alt+V", lenient read, one-time migration from the old presets), ids 1-8 (6 lyrics card, 7 new sticky note, 8 sticky note list), `app_hotkeys.rs` and the generalised PrtSc hook table, the recorder hook (swallow, self-tracked modifiers, drain, safety timers), pausing while recording, the Tombol pintas section
- [[widgets]]: desktop widgets (2026-11-04): phase 1 is the layered-window host, sticky notes sharing `notes.json` with the Notes page (paper, checklist, snap, per-monitor place, roll, pin, cat peek, keyboard through a hidden EDIT), Settings -> Widget, five ways to make a note, hotkey id 7, timers 45 / 46, `--demo-widgets`; v2 run A: the drag bug (the hidden EDIT swallowed presses) and the 36 dip strip, Alt-drag, reminders with the one-shot timer 47 and a sticky island notice, fonts, rulings, tape and pin, stamps, tilt, progress and confetti, cat moods, templates, see-through; **Run B (finished 2026-11-04):** clipboard fill ("Isi dari clipboard") and OLE drops of text, links and files, stacks (Shift + drag, wheel or arrows, "Keluarkan dari tumpukan"), tuck to the edge ("Selip ke tepi"; since 2026-11-05 the tab pops out as a scale and fade, not a slide), Kirim ke HP, and how they meet (a drop lands in the shown sheet, a drag brings a tucked note out, the cap counts sheets, menu ids 24 / 25 / 26, 16 demo notes); **format and images (2026-11-05):** a rich-text bar under the note while it has the keyboard (bold, italic, underline, strike, bullets, checklist, highlighter and text colour, images), `styles` runs and `images` names in the sticky part, runs rebased centrally in `NotesStore`, `Vale.NoteRich` clipboard format, up to 4 images (WIC import on a worker, note-local `WM_APP + 2`, `valid_name` security, release by known-set diff and a startup sweep), the phone page shows the formatting, 18 demo notes
- [[d2d-bitmaps-vs-pixels]]: gotcha: D2D bitmaps die with the render target, decoded pixels do not; the sticky note image cache keeps pixels across a resize (nearest size while dragging, exact decode once settled) and the sweep also removes stale import `.tmp` files (2026-11-05, details in [[widgets]])
- [[sticky-note-list]]: the "Daftar catatan tempel" window (2026-11-05): one row per sticky note (every stack sheet too) with a search box, opened by the new list button in a note's header (`Btn::List`, header buttons now 20 dip with a 1 dip gap), the tray item or the eighth hotkey row (id 8, `sticky_list`); pure rules in `widgets/list.rs`, the layered window in `widgets/list_win.rs`, `app_note_list.rs`; search through the hidden EDIT, Delete key only for a keyboard-picked row, show and focus (untuck, unroll, stack sheet shown, `bring_front` / `lifted`), delete with a 5 s undo strip (snapshot rules, `NotesStore::restore`, timer 49 one-shot), `--demo-note-list`
- [[sticky-note-on-top]]: every new sticky note (and the list's show) is lifted over other windows until another window is activated: exclusive, ends on the note's deactivation, a popover or dialog closed away and the island losing activation (deferred when `App` is borrowed), placed under a topmost island; Notes card sticky-paper icon and one-item right-click menu (2026-11-07)
- [[settings-window-size]]: Settings resizes and maximizes (min 720x520, column capped at 760 and centred, `layout_in`), the place is remembered (`settings_window`, screen coordinates), drag auto-scroll
- [[settings-structure]]: Settings in ten sections (Umum, Tampilan, Kucing, Panel, Widget, Clipboard, Tombol pintas, Fitur, Integrasi, Tentang), titled cards, renamed labels, which sections scroll (v3.8; Umum no longer wraps)
- [[self-update]]: Check for updates (tray, Tentang), release JSON, SHA-256, swap the running exe (rename to .old), `--updated` restart with the 60 s mutex-handle wait, rollback, sticky offer with Update / Nanti (re-offered at every check), exit-crash root cause (WARP objects released before exit, `end_process`), 24 h wall-clock timer (35), never in debug or demos (v3.11, v3.15.1)
- [[recording-audio]]: audio recording, phase 1 (2026-11-07): mic, PC audio or both to `.m4a` + sidecar on a QPC timeline, AAC 96 kbps fragmented MP4, threads only while recording, TIMER_REC 60 / TIMER_REC_LEVEL 61, hotkey 9 `Ctrl+Alt+R`, Rekam page, tray dot, silent recordings not kept
- [[recording-controls]]: mute per source (zeros before the timeline), mute hotkey id 12 `Ctrl+Alt+M` registered only while recording, mic/PC-audio device choice with live switch and fall-back, echo cancellation through the speech category (not communications: ducking)
- [[notch-rec-mute]]: the closed REC notch's level groups are mute buttons; hover never opens the panel (`rec_mute_hot`), cleared only on leave / stop / pause / fullscreen, never on a view change
- [[recording-hand-checks]]: phase 5 live captions: what the agent verified live and the owner's checks (charger, Mic juga with a call, real fullscreen video, CPU-only, 30+ min session); phase 1 is done, and the phase 2 transcript checks (playback heard, summary prompt pasted, .md opened, echo on a real call) plus what the 2026-11-09 polish live check left to judge by eye; the checks still to do by hand on the real app (dropdown with a mouse, light theme, focus, real voice with echo cancellation, headset mid-recording, sleep)
- [[transcript]]: recording phase 2 (2026-11-09): offline Whisper (turbo with a GPU, small without), auto after each recording or by hand, one-job queue with holds and `queue.json`, downloads only on a click, echo filter, file naming, the transcript window and player; polish 2026-11-09: stale start notice dropped, two-click Hapus, length probed from the audio, live "Saya" held until "Peserta" settles
- [[screen-recording]]: recording phase 3 (2026-11-11): a monitor, a window or an area to `.mp4` through the screenshot picker and a 3-2-1 notch countdown, mic and PC audio mixed to mono (sum + soft limiter, `Recorder::start_mix`), `.mp4.part` fragmented, pause spans in the clock, Vale's windows out of the video only while recording, hotkey row 10 `Ctrl+Alt+Shift+R`, Esc id 13, timer 70, `WM_APP + 90/91`, the Layar tab, Settings 30/60 fps and video folder, transcripts of videos, the live check; phase 4 (2026-11-11): `Ctrl+Alt+Z` zoom in the video (hotkey 11, only while recording the screen), `Ctrl+Alt`+wheel 1.5-4× through a hook thread that exists only while zoomed (`WM_APP + 92`), the level in the notch, cursor highlight and click ripples with Settings switches
- [[live-captions]]: recording phase 5 (2026-11-11): offline live captions in a never-activated caption card (stays over fullscreen) and an optional notch line; own capture (PC audio, "Mic juga" adds Saya/Peserta), Transkrip engine and model, interim grey lines, 15 s backlog cap with a gap and a one-time slow hint, soft `Hold::Captions`, battery pause with "Tetap jalan", one model load at a time, hotkey 14 `Ctrl+Alt+C`, tray, Rekam switch, Settings card, timer 66, `WM_APP + 87`, `ui/card_surface.rs` shared with the lyrics card
- [[menu-pop]]: Vale's own themed dropdown (`ui/menu_pop.rs`), first for the recording devices; `WM_APP + 81` Done, `menu_open` holds the panel
- [[lyrics-lookup]]: LRCLIB 503 + Retry-After retry (30 s budget), step order (field search, get, free-text search), None vs transient, the Artist - Title split, tight-window consensus pick of the best-synced version, intro shift for a longer video, `.lrc2` / `.none4`

## Gotchas

- [[studio-light-secondary-text-contrast]]
- [[studio-font-loading]]
- [[demo-data-time-of-day]]
- [[studio-tabular-figures-word-gaps]]
- [[cli-panel-keyboard-focus]]
- [[media-external-pause-causes]]
- [[cli-explorer-com]]
- [[clipboard-privacy]]
- [[install-script-follows-latest-release]]
- [[shot-editor-paint-order]]: Direct2D paints in call order; the editor's footer background must come before the buttons in it (the Discard bar)
- [[taskbar-icon-from-exe]]: the Win11 taskbar shows the exe's cached icon, not WM_SETICON; the installer and the app tell the shell
- [[d2d-dc-target-full-copy]]: a Direct2D DC render target copies the whole bound DC rect at BeginDraw and EndDraw whatever the clip; the island draws on a WIC bitmap target and copies only the dirty rows
- [[test-spawns-no-window]]: tests spawn console children with CREATE_NO_WINDOW and never show windows (editor `ON_SCREEN`), or Windows Terminal windows pop up
- [[controlled-folder-access]]: Windows' ransomware protection refuses unsigned Vale in Documents/Videos with "not found"; recordings fall back to `%USERPROFILE%\Vale\Rekaman` and the default moves there
- [[transcript-defender-toast]]: the first GPU transcript can raise a Windows Security "Unauthorised changes blocked" toast (AMD Vulkan driver under Controlled folder access); it still completes, allow `vale.exe`
- [[light-accent-contrast]]: `#FFFBF5` on the plain light accent is 4.26:1; use `primary_fill` / `segment_fill` for any accent fill with text; the Confirm ("Yakin?") hover/pressed fill is `confirm_fill`
- [[media-foundation-dynamic-load]]: Media Foundation is loaded with LoadLibraryExW, never imported, or vale.exe will not start on Windows N; one loader now, `src/mf.rs` (`MfSession`: COM MTA + MFStartup)
- [[wasapi-capture-timestamps]]: GetBuffer's QPC position is in 100 ns units; loopback sends no packets in silence
- [[exclude-from-capture-in-context]]: an out-of-context WinEvent hook lets a new window show for a frame before `WDA_EXCLUDEFROMCAPTURE`; an in-context hook on the own process (exe module) sets it inside `CreateWindowEx` / `ShowWindow`
- [[mp4-silent-audio-track]]: an `.mp4` whose audio track never got a sample cannot be read back (0xC00D36C4); fill silence until the first sample
- [[tray-dark-menu]]: the tray menu follows the Windows app mode via uxtheme ordinals 135/136/104 (AllowDark), not Vale's window theme

## Conventions

- [[studio-bata-logo-sources]]
- [[screenshot-capture-script]]
- [[logo]]: the cat head is the logo (icon, tray, Settings, README); Windows icon caches
