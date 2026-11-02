---
tags: [decision, theme, settings, editor]
date: 2026-11-02
---

# Terminal surface: neutral dark windows

When the chosen island surface is **Terminal** (#1B1D1F, a flat neutral
near-black, see [[cat-colours-and-surfaces]]) and the window is dark, every
Vale window that uses the shared `WindowPalette` is neutral too: Settings, the
reminder / notes / event editors with their calendar and time popovers, and the
screenshot editor. They would otherwise sit on the warm brown (#1E1814) that
the island no longer has.

## Rule

- Terminal + dark only. Dark means `WindowTheme::Dark`, or `System` while
  Windows is dark.
- Light is the same palette whatever the surface. Every other surface (Auto,
  Espresso, Graphite, Sea, Night, Plum) keeps the warm dark palette, pixel for
  pixel (a test compares `window_for` with `window` for every accent, surface
  and mode).
- Not touched: the island, the cat, the native tray menu, the accents, the CLI
  terminal constants `TERMINAL_*`.

## Values (`theme::window_for`, pinned by a test)

The neutral palette lifts #1B1D1F by the steps the warm palette takes above
its own bg, equal on every channel.

| Token | Value | Step | Text / secondary / tertiary on it |
|---|---|---|---|
| bg | #1B1D1F | the island's Terminal fill | 14.56 / 7.49 / 2.97 |
| card | #232527 | +8 | 13.24 / 6.82 / 2.70 |
| popover | #252729 | +10 | 12.91 / 6.64 / 2.63 |
| field | #27292B | +12 (the island's Terminal raised) | 12.57 / 6.47 / 2.56 |
| button, hover | #2C2E30 | +17 | 11.74 / 6.04 / 2.39 |
| button_hover | #353739 | +26 | |
| secondary | #A9ADB2 | the island's Terminal secondary | |
| tertiary | #646769 | placeholders | 2.97 on bg (warm #6E6155 on #1E1814: 2.93) |

Accent, `on_accent`, selection (accent at 16 %) and error (#EF7A6E, 6.18:1 on
bg) are as in the warm palette; border is the text colour at the same low
alpha.

**Text stays #F4EDE4** (decision): the island's text is the same on every
surface, Terminal included, so the windows keep it. It reads 12.57 to 14.56:1
on every fill. Tertiary is the same quiet placeholder class as the warm one
(under 4.5:1, within 0.15 of the warm contrast); the tests check this.

A test also checks neutrality: bg, card, field, popover, button, button_hover,
hover, secondary and tertiary each spread at most 9 between channels and none
has red above blue.

## API

`theme::window_for(accent, surface, mode, system_light)`. `window(accent, mode,
system_light)` keeps its signature and gives the warm one (it is
`window_for(.., Surface::Espresso, ..)`), so `settings_controls.rs` tests are
untouched. `current_window()` calls `window_for(t.accent, t.surface, t.window,
system_light)`; `Theme.surface` is always concrete (`Theme::of` resolves Auto,
and Auto never gives Terminal), so `surface == Terminal` means the config says
Terminal. `pack` / `unpack` already carried the surface.

## Three caches that assumed "the palette depends only on light / dark"

1. **Editor brushes** (`ui/editor.rs` `brushes`): were two slots keyed by
   `p.light`, so the editor background and the field brushes
   (WM_CTLCOLOR*, popover fields) stayed warm after a switch. Now keyed by the
   `(bg, field)` pair: three slots (light, warm dark, neutral dark), created
   once and kept for the process lifetime (`brush_slot` is the pure lookup,
   tested).
2. **Settings title bar** (`ui/settings.rs` `restyle`): re-applied the DWM
   caption colour only when light flipped, so warm dark to Terminal dark kept a
   warm title bar. `styled: Option<(light, bg, text)>` now drives it
   (`style_changes`, tested); the EDIT controls' `SetWindowTheme` stays gated on
   a light change only.
3. **Screenshot editor** (`shot_edit/window.rs`): stored its palette at open and
   never restyled. `restyle_open()` sets `e.p = current_window()` for every open
   editor under the `EDITORS` borrow (skipped if busy), then, outside the
   borrow (DWM can re-enter `wnd_proc`), re-applies the frame and invalidates.
   The editors live on the UI thread (the capture overlay opens them there).
   Nothing else there caches a colour from the palette.

The popovers (`editor_popover.rs`) read `current_window()` on every paint and
cache nothing per mode.

## Live switch, no restart

`App::apply_theme` (surface, accent or window mode changed) and
`on_system_theme_changed` (Windows light / dark) call
`editor::restyle_open_editor()`, `shot_edit::window::restyle_open()` and
`settings::restyle()`. A surface change flips `Theme`, so it goes through
there, same path as [[ui-font-option]].

## Hand-edited config.json

No new code. `surface` is island-owned and the file is not watched, so a hand
edit applies at the next start, like the other island-owned keys (see
[[ui-font-option]]).

## Demo

`DI_SURFACE=terminal` gives neutral windows in `--settings`, `--demo-editor`,
`--demo-shot-editor` and `--demo-snap`; `DI_WINDOW` (`dark`, `light`, `system`)
and `DI_ACCENT` apply to the last three too (`theme::theme_from_env`, debug
builds only; unset behaves as before: default accent, Espresso, system mode).
`--settings` already read `DI_SURFACE`.

Related: [[studio-editor-frame]], [[settings-structure]], [[cat-colours-and-surfaces]]

Listed in [[index]].
