---
tags: [gotcha, island, win32, input]
date: 2026-11-14
---

# A playing island lets clicks through with WS_EX_TRANSPARENT

While a story plays, the parts of the window outside the island must not eat
clicks meant for the window below. `WM_NCHITTEST` returning `HTTRANSPARENT`
does not work here: it only passes the click to windows of the same thread.
The island toggles `WS_EX_TRANSPARENT` instead (`IslandWindow::set_click_through`,
read-modify-write of `GWL_EXSTYLE`, only on change) while the pointer is not
on the island.

The flag is derived from the play slot at one place, after the slot is
dropped (`advance_slot` in `app.rs`), so it cannot stay on after a play ends;
off the idle view (a panel or notice arrived mid-wind-down) it is always off.
A stuck flag would make the island unclickable. Mouse buttons are read with
`GetAsyncKeyState` because a transparent window gets no button messages. See
[[mascot-play]].
