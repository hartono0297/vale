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
- [[exe-size]]: size measured per change; icon small sizes as PNG (-38 KB), opt-level z option (-326 KB) left to the owner
- [[studio-icons]]
- [[studio-calendar-day-view]]
- [[studio-media-progress-timer]]
- [[studio-editor-frame]]
- [[studio-reminder-editor]]
- [[bata-robot-expressions]]
- [[bata-idle-power-budget]]
- [[bata-morning-greeting]]
- [[greeting-periods]]: the cat greets once per period (5) per day by local time, not just mornings
- [[lyric-notch-width]]
- [[compact-padding]]: one 16 dip inset (COMPACT_INSET) for the compact views; shapes grow (media +12, pomodoro +22, lyric +10 with the robot), text room unchanged
- [[notch-size-and-fill]]: Tampilan -> Notch: width/height sliders (160-300 x 28-44), solid or see-through background, the panel stays solid
- [[settings-appearance-scroll]]
- [[media-click-guards]]
- [[media-diagnosis-log]]
- [[media-session-priority]]: every media session is ranked and the best is shown (playing, music, Windows' current, the one shown before); music = known player or artist + album; per-session subscriptions, timeline and thumbnail only for the shown one; commands go to the shown session by key
- [[cli-explorer-folder]]
- [[system-page]]
- [[cleaner]]
- [[mole-mode]]
- [[clipboard-history]]
- [[notification-history]]: the Notifikasi page (13th, after Clipboard): a capped, optionally encrypted history of the toasts, exclusion rule, TIMER_NOTIF_SAVE (34), keyboard only by click, logo path, notch toast logo + app eyebrow, browser site (file v2) (v3.9)
- [[clipboard-notice]]: a 3 s preview in the notch on every external copy (text lines, thumbnail, files), skip rules, supersedes "Disalin untuk semua salinan", no timer
- [[screenshot-preview]]: preview matching of the Windows file; Simpan never writes a duplicate (v3.6)
- [[screenshot-snap]]: own region capture that snaps to windows and boxes, the editor after it, UIA budget, own-copy skip, PrtSc key opens Vale's capture (low-level keyboard hook on its own thread, now a hotkey row, nothing in the registry)
- [[panel-pages]]
- [[vale-rename]]: the Vale rename, what keeps the old name, and the 2026-11-01 repo rename to hartono0297/vale
- [[google-built-in-client]]
- [[github-pages]]: site/ goes to GitHub Pages through the Actions source (Pages cannot serve site/ directly)
- [[site-landing]]: the landing page, direction B "dark stage" (dark by default, light toggle) with a working notch: section order, Susun section with a sticky scroll scrub and the pinned-only arrange notch state, 14-tab dock, one scroll loop (hero, Susun, lyric, fitCheck), tokens and contrast in both themes, the shared header/footer/legal markup contract, no-script and reduced motion, `scripts/check-site.py`, what was verified
- [[v3-1-handoff]]: where v3.1 stands, how to continue on another PC
- [[open-mode]]: Mode buka (Hover, Hover & tetap, Klik) and notices with a button
- [[panel-linger-and-motion]]: the panel stays open 4 s after an action; every page and sub-view change moves (slide, fade, reveal), reduced motion, budget
- [[scroll-volume]]: the wheel over the music notch / cover turns the master volume 2 % a notch
- [[lyrics-sync]]: position rate, resync tolerance, lookahead, no manual offset since 3.9, lrclib title clean + search fallback
- [[shortcut-tiles]]: Pintasan tiles chosen in Settings (page jumps, Vale Settings, own tiles added in a form), drag to reorder
- [[gestures]]: middle click, swipe and sideways wheel on the music views; sideways swipe turns panel pages (v3.6)
- [[v3-2-handoff]]: where v3.2 stands, what is unverified, owner steps
- [[v3-3-handoff]]: v3.3 (icon refresh, stay-behind rise, shelf drop, file search), owner checks, minors left
- [[v3-5-handoff]]: v3.5 (editor close bar, Devices, notch size and fill, shelf motion, clipboard notice), what was cancelled, owner checks
- [[readme-structure]]: README with gallery, install and uninstall, credits in THIRD-PARTY-NOTICES.md; long guide in docs/guide.md; APNG tours
- [[stay-behind]]: experimental setting, island below ordinary windows, rises at the top edge with a short fade and slide (v3.3)
- [[shelf]]: file shelf page, OLE drop target, drag out (paths only, in memory); a drop latches the panel (v3.3)
- [[shelf-motion]]: shelf cards animate in (200 ms), out as ghosts (180 ms) and across (220 ms), no timer, per-draw alpha, reduced motion (v3.5)
- [[search]]: quick search on a hotkey: apps, calculator, Windows settings, web; back button returns to the previous page
- [[search-files]]: files and folders by name in six user folders, own walker with caps, icons/sizes/counts from a background worker
- [[mixer]]: Mixer page, per-app volume through a Core Audio worker thread
- [[devices]]: Devices page (Perangkat), safe eject of flash drives, external disks and cards through a worker thread, no polling
- [[notch-peek-eyes]]: with stay-behind off, the notch shrinks to the cat's two eyes while the pointer is near it over a window, comes back on a 500 ms rest or a click; a fast entry (also caught on the island's WM_MOUSEMOVE) shrinks too; 320 ms ease-in-out eyes morph; shares TIMER_POINTER (33); since v3.11 the eyes look toward the pointer ("Ikuti kursor") and blink every 3-6 s on TIMER_BLINK
- [[cat-looks-at-pointer]]: the idle cat turns head, ears and eyes toward the pointer sideways, eased on the frame clock, clamp checked by a test; "Ikuti kursor" turns it off (v3.8)
- [[cat-colours-and-surfaces]]: four coats (Terracotta, Calico, White, Black), five island surfaces plus "follow the cat", contrast numbers, lenient config (v3.8)
- [[cat-accessories]]: seven accessories, the one-silhouette paint order (rim, ears and head, calico patches cut to the whole silhouette, marks, accessories), the rejected mockups, the clamp test (v3.8)
- [[eq-follow-audio]]: the compact view's equalizer bars follow the real music (WASAPI loopback, five bands, per-band auto-gain, `eq-capture` thread only while `TIMER_EQ` runs and "Equalizer ikut irama" is on, sine fallback)
- [[cat-dances-to-music]]: the music and lyric cats nod to the bass (600 ms fallback beat) with a floating note, "Joget ikut musik", `dance_on` gate, note clamp under short notches, TIMER_EQ now also in the lyric view while dancing (v3.10)
- [[cat-types-while-working]]: work running (Claude, CLI, Mole) gives the idle cat a laptop and typing paws instead of Think, eyes-only glance at the pointer (1.2 s rest), island grown 12 dip on the left (`idle_shape_typing`, `idle_layout_lead`), `TIMER_TYPING` (40) one-shot, gate, demo scenes (2026-11-01)
- [[arrange]]: Susun (v3.10), the window layouts: tree model with built-ins, gap maths, pick step with a click-through-gotcha overlay, mover on a worker with frame compensation and undo, saved arrangements, hotkey id 4, Page::ALL 14 / Shortcut::ALL 22, own pick overlay looks fullscreen to Windows (check skipped while it is up)
- [[hotkeys]]: the five global hotkeys as recordable combos (`config.hotkeys`, text form "Ctrl+Alt+V", lenient read, one-time migration from the old presets), ids 1-5, `app_hotkeys.rs` and the generalised PrtSc hook table, the recorder hook (swallow, self-tracked modifiers, drain, safety timers), pausing while recording, the Tombol pintas section
- [[settings-window-size]]: Settings resizes and maximizes (min 720x520, column capped at 760 and centred, `layout_in`), the place is remembered (`settings_window`, screen coordinates), drag auto-scroll
- [[settings-structure]]: Settings in nine sections (Umum, Tampilan, Kucing, Panel, Clipboard, Tombol pintas, Fitur, Integrasi, Tentang), titled cards, renamed labels, which sections scroll (v3.8; Umum no longer wraps)
- [[self-update]]: Check for updates (tray, Tentang), release JSON, SHA-256, swap the running exe (rename to .old), `--updated` restart with the mutex-handle wait, rollback, offer once, 24 h wall-clock timer (35), never in debug or demos (v3.11)
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
- [[tray-dark-menu]]: the tray menu follows the Windows app mode via uxtheme ordinals 135/136/104 (AllowDark), not Vale's window theme

## Conventions

- [[studio-bata-logo-sources]]
- [[screenshot-capture-script]]
- [[logo]]: the cat head is the logo (icon, tray, Settings, README); Windows icon caches
