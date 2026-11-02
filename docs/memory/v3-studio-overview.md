---
tags: [overview, decision, studio, v3]
date: 2026-10-27
---

# v3 Studio: overview

Version 3.0.0 is the Studio redesign (`docs/design/rencana-v3.html` is the
plan, `studio-ui.html`, `editor-acara.html`, `pengingat-cli.html` and
`studio-robot.html` the designs). This note is the map; each fact lives in
the note it links to.

## Look

- **Palette.** Warm accents and surfaces replaced the old ones; old names in
  `config.json` are aliased, not reset. [[studio-palette-accent-aliases]]
- **Contrast beats the mock-up.** Light secondary text was nudged to pass
  4.5:1; re-check any pasted design value. [[studio-light-secondary-text-contrast]]
- **Typefaces.** DM Sans (text and titles, 600 upright for titles since v3.1)
  and DM Mono, embedded and subset by a pinned, reproducible script.
  [[studio-font-sources]]
- **Loading them.** DirectWrite and GDI each need their own path, with
  fallbacks. [[studio-font-loading]]
- **Tabular figures on digits only**, or DM Sans widens word gaps.
  [[studio-tabular-figures-word-gaps]]
- **Icons** are a Direct2D vector set, not Segoe glyphs. [[studio-icons]]
- **The Bata logo** has three copies that must match.
  [[studio-bata-logo-sources]]

## Island and panel

- **Calendar** day view pages through hour windows; the title sets where the
  arrows sit. [[studio-calendar-day-view]]
- **Music progress** redraws once a second only while the Music page shows a
  playing track; the equalizer has 5 bars. [[studio-media-progress-timer]]
- **CLI in the panel.** The separate CLI window is gone; the island takes the
  keyboard only after a click on the terminal and hands it back.
  [[cli-panel-keyboard-focus]]

## Editors

- **Frame.** Frameless DWM window drawn in Direct2D, with real borderless
  EDITs and popovers. [[studio-editor-frame]]
- **Reminders.** Summary line, weekly reminders on several days, quick-time
  shortcuts (+30 min, +1 h, tonight, tomorrow morning).
  [[studio-reminder-editor]]

## Bata, the robot

- **Faces** follow one priority list; panel open means no robot.
  [[bata-robot-expressions]]
- **Power.** Only one-shot timers (a blink every 4–7 s while awake); asleep, only the 1.8 s `TIMER_ROBOT` wake poll (one-shot, re-armed) runs while the Sleep face is on screen.
  [[bata-idle-power-budget]]
- **Greeting**, once per time-of-day period (five) per day since v3.8, never
  spent on a locked screen. [[greeting-periods]], [[bata-morning-greeting]]
- **Settings** in eight sections since v3.8, the cat on its own Kucing page
  ([[settings-structure]]; older: [[settings-appearance-scroll]]).

## Demo and screenshots

- `--demo` scenes are built around the current time; check early and late.
  [[demo-data-time-of-day]]
- Still open: the 3.0 screenshots in `assets/screenshots` show the old
  italic titles and need a fresh capture for 3.1 ([[studio-font-sources]]).

Listed in [[index]].
