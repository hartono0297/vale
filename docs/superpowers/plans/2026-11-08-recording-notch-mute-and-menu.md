---
tags: [plan, recording, notch, ui]
date: 2026-11-08
---

# Mute from the notch, and Vale's own device dropdown

Asked by the user on 2026-11-08: "bisa ga mute nya ada di notch, user ga perlu
click perbesar dlu" and "bisa ga bikin dropdown pilihan audio sesuai theme
vale". The mockup was approved ("Oke, gas"). Builds on [[recording-audio]] and
[[recording-controls]] (`docs/memory/`). Two independent tasks, each in its own
worktree off `main`, test-first, merged one after the other.

## T1. Mute from the closed notch (branch `rec-notch-mute`)

- The REC compact view (`paint_record_compact`, `View::RecordCompact`) gets
  one hit region per recorded source over its level bar group: new
  `Hit::RecMuteCompact(Source)` (or reuse `Hit::RecMute(Source)` if the click
  path allows), pushed from the render like `NoticeClose` / `ToastSlot`.
  Only recorded sources get a region (a mic-only recording has one).
- **Hover over a group** shows a soft rounded pill behind it and swaps its
  bars for the source icon (`Icon::Mic` / speaker; when muted the crossed
  icon stays, tinted `theme::REC_RED` on a dark red pill). Hovering a group
  must **not open the panel** in Hover or Hover & stay mode: a pointer that
  rests on a mute region does not count as hover for opening (the way
  `ToastSlot` / `hover_blocked` keep the toast logo from opening it). Moving
  from a group onto the rest of the notch opens the panel as usual.
- **Click on a group** toggles that source's mute (`Effect::RecMute(source)`,
  the existing path), in every open mode, with no panel opening and no
  linger. A click elsewhere on the REC view keeps today's behaviour.
- The hot (hover) state is drawn without a new timer: redraw on pointer move
  into and out of a region, as the notice ✕ does (`track_close_hot`).
- Fullscreen, Motion Off and reduced motion: no animation needed; a hidden
  view has no regions.
- Tests: the regions exist only for recorded sources and sit over the bars;
  hit testing maps them; state tests for each open mode (click toggles mute,
  panel stays closed, hover over a region does not open, hover elsewhere
  does); demo scene `rec-hover-mic` showing the hot pill.

## T2. Vale's own dropdown for the device choice (branch `rec-device-dropdown`)

- New reusable themed popup list `src/ui/menu_pop.rs`: a small layered,
  topmost, frameless popup window drawn with Direct2D in Vale's palette
  (follows the current theme and island surface, light and dark, as the
  editor popovers and `widgets/pop.rs` do), rounded corners, rows with an
  optional check (accent colour), separators, hover highlight, long labels
  ellipsized, width clamped (min ~220, max ~380 dip), placed under the anchor
  rect and clamped to the monitor work area (flips above when there is no
  room below), DPI aware.
- Keyboard: Up/Down (skipping separators), Home/End, Enter picks, Esc closes;
  closes on deactivation or a click outside. One instance at a time.
- Result delivery without holding the app borrow: the popup posts the pick to
  its owner window (a `WM_APP` message with a boxed choice, as
  `editor_popover` does) or returns `None` when dismissed.
- Model: reuse `tray::device_items` / `DeviceItem::{Entry, Separator}` and
  `device_choice`; move them next to the popup if that reads better.
- Replace the native `tray::device_menu` at both call sites: the Rekam page
  (`open_rec_device_menu` in `app.rs`, opened under the clicked device name)
  and Settings → Fitur → Rekaman (`After::PickRecDevice` in `settings.rs`,
  opened under the row's "Ganti" button). The tray's own menus stay native.
- Endpoints are still read when the dropdown opens; nothing at idle.
- Tests: pure layout (row rects, separators, ellipsis width, clamp and flip
  against a work area), keyboard navigation over separators, the pick
  mapping; a demo or `--settings` path to screenshot it in light and dark.

## Then

Whole-branch review of both (most capable model), live check (hover and
click the notch groups in Hover mode; the dropdown on the Rekam page and in
Settings, light and dark), merge into `main` one at a time, `cargo test
--locked`, memory and README, push, clean up.
