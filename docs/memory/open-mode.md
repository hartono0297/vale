---
tags: [decision, gotcha, panel, hover, notices, v3.2, v3.3]
date: 2026-11-03
---

# Open mode and notices with buttons (v3.2 Run 1)

Settings → Umum → **Mode buka** (`config.open_mode`, `OpenMode`): `Hover` (the
default, as always: opens under the pointer, closes when it leaves),
`HoverStay` (opens on hover, stays until a click outside the island) and
`Click` (opens on a click on the compact island, stays until a click
outside). A typo in the file reads as `Hover`; the field is the island's own
(`merge_owned`).

## How it works (`state.rs`)

- `latched` is "kept open". `panel_open()` is `pointer || latched || editor ||
  CLI input || clipboard`. `pointer` is hover, unless the mode is Click.
  `collapse()`, a fullscreen app and a mode change clear the latch.
- HoverStay latches at the end of `apply` (`hover_stay_latches`) only once
  the panel is what shows: hover on, no `current` notice over it, not
  `hover_blocked`, not shut, not fullscreen. A pointer passing over a notice
  never leaves a panel latched behind it; one that stays until the notice
  ends gets the panel, kept.
- `Event::Dismiss` (a button went down outside the shape) closes a latched
  panel. Under the pointer it stays shut until the pointer comes back
  (`collapse()`), so it never reopens at once.
- Click mode: `Hit::Body` on Idle, MediaCompact, MediaLyric, PomodoroCompact
  **and a sticky notice** (Claude waiting, a due reminder) opens; the strip
  keeps the sticky notice and its own click (Selesai). Exception: the due
  reminder's Done/Selesai chip on the closed notch is its own hit
  (`Hit::ReminderDone`, see "Notices with a button"): one click marks it done,
  the panel does not open. A temporary notice
  (`current`) is still dismissed by a click. `hover_opened` starts the media
  guard, and a latched panel keeps it even before the hover dwell arrives
  ([[media-click-guards]]).
- A file drop on the island latches the panel like a click (v3.3,
  `Event::ShelfDropped`, see [[shelf]]); in Hover mode the drop's own visit
  does not close it, the next one hands the panel to hover.
- What else holds the panel (an editor, the CLI input, the clipboard search)
  survives a click outside, as it did before.

## Outside clicks without a hook (`app.rs`)

`TIMER_OUTSIDE` (24) runs **only while** `outside_click_wanted()` (latched,
open, not fullscreen) and the session is not locked nor the display off.
Each tick reads `GetAsyncKeyState` for the left, right and middle buttons (so
a swapped pair does not matter). A press is an up-to-down edge **or** the
`0x0001` "pressed since the last read" bit (a click quicker than the poll).
It is outside unless the island took it itself: `WM_LBUTTONDOWN`,
`WM_RBUTTONDOWN` and `WM_MBUTTONDOWN` mark `OutsidePress::press_inside`
(input messages are handled before `WM_TIMER`), so the cursor is no longer
hit-tested at poll time (the pointer may have moved). A press already down
when the polling starts is not an edge (`reset`, which also spends old
`0x0001` bits); an inside mark the poll never used is dropped once the
buttons are up. Pace: 40 ms, 120 ms after a second without input
(`GetLastInputInfo`, `outside_poll_ms`), back to 40 ms on the next poll with
input or a `WM_MOUSEMOVE` over the island; `SetCoalescableTimer` slack ≤ 30
ms. Rejected: `SetCapture` (swallows the click and fights `TME_LEAVE`), a
global low-level mouse hook (fires on every move and is removed silently if
the UI thread is slow), deactivation (the window never activates). It never
swallows a click, so the press-release pairing of [[media-click-guards]] is
untouched.

## Notices with a button

Root cause of "hover gets in the way of Selesai": `view()` checked
`panel_open()` before the sticky notice, so a 120 ms dwell opened the panel
and the reminder shrank into a small strip that moved during the opening.

- `notice_has_action`: Reminder, MoleDone, SystemLow, Pomodoro, Screenshot,
  a `ShotSaved` with a path (its click reveals the file, `Effect::RevealFile`)
  and an App notice with an app id. A hover that starts on one sets
  `hover_blocked`: the panel stays shut for that visit.
- A sticky reminder blocks **only the first visit after it appeared**
  (`reminder_seen`); later visits open the panel with it in the strip, as in
  v3.1 (a reminder waiting for hours must not lock the panel away).
- A wheel notch peeks at the panel only over the sticky reminder
  (`current` is none); that notch is spent on opening, not on turning the
  page. Over a temporary notice the wheel is the notice's: nothing opens or
  latches behind it.
- Its expiry waits only while the pointer's visit **began on it**
  (`visit_on_notice`, `notice_held`); once it leaves, `SHOT_LEAVE_GRACE` (2 s)
  first. A notice that arrives under a pointer already there (over an open
  panel, in any mode) runs its normal time, then the panel is back.
- A click that dismisses it does not pop the panel under the pointer, except
  SystemLow, MoleDone and MoleFailed, whose click leads to the panel; in
  HoverStay and Click that click also latches it (`open_by_click`), so the
  panel opens and stays.
- The due reminder notice's Done/Selesai chip is its own hit,
  `Hit::ReminderDone` (the painted chip rect plus `NOTICE_CHIP_HIT_MARGIN`, 4
  dip, registered in `paint_notice`, never in the strip, which keeps
  `Hit::Strip`). One click marks the reminder done in every mode: the panel
  does not open and does not latch, no linger starts. Before, the pill had no
  region, so in Click mode the click was `Hit::Body`, opened the panel, and
  needed a second click on the strip. A click elsewhere on the reminder in
  Click mode still opens the panel with it in the strip; in Hover mode a body
  click still dismisses.
- The sticky update offer ("Vale X.Y.Z tersedia", Update and Nanti buttons)
  shares the reminder's first-visit rule: only the first visit after it
  appeared is blocked, later visits open the panel with the offer in the
  strip ([[self-update]]). Its body click opens the panel in Click mode, never
  installs.
- **Gotcha:** Claude waiting and a failed Mole run are sticky and only inform;
  they must NOT block hover, or the panel could not be opened while they
  show. Only a sticky notice with buttons (the reminder, the update offer) blocks.
- A sticky notice that arrives over an open panel changes nothing: the panel
  stays and shows it as a strip.

Esc closing a kept panel is not global (a global Esc would close the island
from any other app); it stays inside the CLI and search inputs.

**Not verified live:** the `0x0001` bit is shared with other processes that
call `GetAsyncKeyState`; a quick click whose bit another app spent is missed
at the 120 ms pace (the 40 ms pace usually still sees the edge).
