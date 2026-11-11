---
tags: [gotcha, ui, contrast, light-theme]
date: 2026-11-09
---

# Light windows: text on the plain accent is below 4.5:1

`#FFFBF5` text on the plain light-theme accent is 4.26:1, under WCAG AA for
normal text. Settings now fills primary buttons with `primary_fill` and the
selected segment of a segmented control with `segment_fill` (5.22:1 and up). Use
them for any accent fill that carries text; the transcript window does
([[transcript]]). One function each, so going back to the lighter accent is a
small change.

The shared `ButtonKind::Confirm` ("Yakin?") label is red on the quiet button
fill; darkening that fill on hover dropped it to 4.31:1 (light) and 4.37:1
(Terminal dark). `confirm_fill` now keeps the rest fill, mixes halfway to the
card on hover and three quarters when pressed (so the pill stays visible), all
4.5:1 or more in three palettes x six accents (2026-11-09).
