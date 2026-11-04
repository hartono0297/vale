---
tags: [decision, robot, power, timers]
date: 2026-11-03
---

# The notch assistant's idle power budget

An idle island must stay cheap: no continuous timer, no frame thread.
Bata adds only one-shots (`app.rs`, planned by `robot::plan_timers`):

- `TIMER_BLINK`: a random 4–7 s wait, then 120 ms with the eyes shut,
  then re-armed. Only while a robot with open eyes shows (compact view or
  the island's own notice), motion is Full, and nothing animates. A
  sleeping, happy or music face runs no blink timer.
- `TIMER_BLINK` also runs while the notch is shrunk to the eyes
  ([[notch-peek-eyes]]), cat or not, on the eyes' own schedule (open 3-6 s,
  shut 120 ms, one blink in four a double with the second shut 260 ms after
  the first starts). Armed only with Motion Full (not Energy Saver) and
  Windows animations on, and never held back by the frame clock (the eyes'
  ease would postpone every blink). Switching between eyes and cat resets the
  state and re-arms or kills the same timer; no new timer id.
- `TIMER_ROBOT`: one-shot at `Brain::next_change`: falling asleep
  (last activity + N min), a Claude session going quiet, a CLI face
  ending. The sleep deadline's "last activity" is now max(island activity,
  last PC input via `GetLastInputInfo`), folded in (`robot::fold_input`,
  `App::fold_pc_input`) at the start of every `refresh` and on every
  `TIMER_ROBOT` fire: one `GetLastInputInfo` per fire, no new timer, so
  input while awake just pushes the deadline out and the timer re-arms there.
  **Wake poll (2026-11-02):** `TIMER_ROBOT` is also a 1.8 s coalescable
  one-shot (`SetCoalescableTimer`, 200 ms slack, so a wake lands within 2 s of
  input) only while the Sleep face is on screen: robot on, Motion not Off, idle
  view, not shrunk, not fullscreen, not locked, display on
  (`robot::wake_poll_on`, `robot_timer_at`). A fire that finds the cat still
  asleep re-arms without a draw (`wake_tick_quiet`); one that finds input
  goes through `refresh`, the face returns to Idle and the pointer, blink and
  typing timers re-plan as usual. Nothing polls while awake. Lock, display
  off/on, fullscreen and unlock re-sync it (`sync_pointer_polls`, the
  fullscreen refresh). The interval is 1.8 s, not 2 s, so interval + slack
  honours "within 2 s". Vale's own synthetic input (media keys, volume wheel)
  also counts as PC input. See [[bata-robot-expressions]].
- Hover lift (140 ms) and the alert's two bounces (2 × 260 ms) run the
  existing `FrameClock` only while they last (`App::animating_at`), and
  the blink re-plans when the clock stops.
- The stay-behind rise ([[stay-behind]]: 180 ms, or a 120 ms fade with
  Motion not Full) runs the same `FrameClock` only while it lasts; no timer.
- The page dock ([[panel-pages]], hover over the dots) runs the `FrameClock`
  only while one of its values moves (`dock.moving()` in `animating_at`); a
  pointer resting on an icon stops it, no timer.
- Panel transitions ([[panel-linger-and-motion]]): the sub-view `Reveal`
  (160 ms), the search's page cross-fade and a page slide are counted in
  `animating_at` like the rise: the `FrameClock` starts when one begins and
  stops when none is left, no timer. The panel's **linger** (4 s after an
  action) adds no timer either: its deadline rides `next_deadline()` on the
  existing `TIMER_DEADLINE`; while it runs, the click-outside poll
  (`TIMER_OUTSIDE`, below) watches so a click outside ends it. Windows'
  animation setting is read at start and on `WM_SETTINGCHANGE`, never polled.

- `TIMER_TYPING` (40, 2026-11-01): the typing cat's paws
  ([[cat-types-while-working]]). A one-shot re-armed with each step's wait
  (`TypingPaws::next`: 110 to 200 ms, 420 ms for about one step in eight, a
  900 ms pause about every 8 s); each tick draws once (unless the frame clock
  runs anyway). That is about 5 to 9 redraws a second, only while the face is
  Typing **and** Motion is Full (Energy Saver folded in), Windows' animations
  are on, nothing is fullscreen, the session is not locked, the display is on
  and the notch is not shrunk (`robot::typing_anim_on`). Otherwise the timer is
  killed, the paws go down (one draw if one was up) and nothing runs. No frame
  clock and no repeating timer for it: `sync_typing_timer` is called from
  `sync_robot_timers` (every `refresh`, and the end of a frame-clock run),
  `sync_pointer_polls` (lock, display) and `follow_pointer_settings`.
  The glance rides `TIMER_POINTER` (below) and the frame clock only while its
  ease moves. `TIMER_BLINK` and `TIMER_ROBOT` already carry the cat's blink
  and `next_change`, which is why the paws have their own id.

- The fullscreen check ([[notch-monitor]], "Follows the notch's monitor") rides
  the existing 1 s `TIMER_SYSTEM` tick and adds no timer. Its z-order walk runs
  only with 2+ monitors and either the foreground window off the notch's
  monitor or the bare desktop in front (its monitor is always the primary), at
  most once per tick, stops at the first window that overlaps the notch's
  monitor and reads at most 1024 windows. Otherwise (one monitor, or a
  non-desktop foreground on the notch's monitor) it adds one `GetSystemMetrics`,
  two monitor lookups and a class-name read of the foreground.

Motion policy (`Motion`, config `robot_animation`):

- **Penuh (Full)**: blink, lift, bounce.
- **Hemat (Saver)**: no blink, lift or bounce; faces and sleep still change.
- **Full with Windows Energy Saver on** (`SYSTEM_POWER_STATUS.SystemStatusFlag
  == 1`, read with the battery every 30 s and on `PBT_APMPOWERSTATUSCHANGE`)
  behaves as Saver.
- **Mati (Off)**: no robot timers at all. Faces still follow state, but
  nothing timed: no sleep, no 4 s CLI Happy/Sad, and a Claude session
  that dies without Done keeps Think until the next event. The sleep
  stepper is disabled in Settings for it.

Robot off: nothing of this runs; widths, timers and drawing are as before.

The System page adds two one-shots (see [[system-page]]):

- `TIMER_SYSTEM_PAGE` (17): 2 s, re-armed on each sample, and only while the
  panel shows the System page. It is killed when the page or panel closes.
- `TIMER_SYSTEM_WARN` (18): 5 min (sooner only to confirm RAM's minute),
  re-armed when its reading returns. It is the only new timer while the
  panel is closed. Its reading runs on the sampler thread, and the island
  redraws only if it raises a notice.

The clipboard history ([[clipboard-history]]) adds no repeating timer
either: the listener costs nothing until a copy; `TIMER_CLIP_RETRY` (20),
`TIMER_CLIP_SAVE` (21) and `TIMER_CLIP_HOLD` (22) are one-shots; a change
while the Clipboard page is hidden draws nothing unless a notice shows;
"Disalin" rides the notice deadline (1.5 s).

The notification history ([[notification-history]]) adds one one-shot,
`TIMER_NOTIF_SAVE` (34): 1.5 s after the last change, coalesced, flushed on
`WM_DESTROY` and `WM_ENDSESSION`. No repeating timer: "Hapus semua" arms
inside the reducer and rides `next_deadline` on the existing `TIMER_DEADLINE`,
relative times ("5 mnt", "Kemarin") are worked out only when a frame is drawn,
and a toast while the page is hidden redraws nothing (the notch notice does
what it always did). Its keyboard input is a hidden EDIT, not a timer; the
poller thread already existed and now only also reads one logo per app per
run.

The screenshot preview ([[screenshot-preview]]) adds `TIMER_SHOT_RETRY`
(23), a one-shot that runs at most twice after a new clipboard image; its
folder looks run on short-lived threads, never a watcher.

The PrtSc key ([[screenshot-snap]]) adds a thread, not a timer: a
`WH_KEYBOARD_LL` hook thread that blocks in `GetMessageW` between keystrokes,
exists only while the setting is on, and whose proc does no work but post one
message.

The equalizer's capture ([[eq-follow-audio]]) adds a thread, not a timer: the
`eq-capture` thread exists after the first time a playing compact media view
shows, blocks in `recv()` while stopped, and runs (a 15 ms wait) only while
`TIMER_EQ` runs and "Equalizer ikut irama" is on. `TIMER_EQ` itself is now
also off while fullscreen and while the session is locked or the display is off
(`screen_away`, re-synced by `sync_pointer_polls`), through
`eq_timer_wanted`.

## Pointer polls (v3.2.1 review, 2026-10-29)

A measured review found ~78 main-thread wake-ups a second at idle (v3.1.0:
~15); the 50 ms halo poll was ~55 of them. The three pointer polls now use
`SetCoalescableTimer` (slack `timer_slack`: half the interval, ≤ 30 ms), are
paced by pure helpers (tested), and all pause while the session is locked or
the display is off (`screen_away`: `WTS_SESSION_LOCK` and
`GUID_CONSOLE_DISPLAY_STATE` via `RegisterPowerSettingNotification`, both
calling `sync_pointer_polls`).

| Timer | Before | Now |
|---|---|---|
| `TIMER_HALO` (30) | 50 ms whenever the island was up, not a panel | none at idle; after a trigger ([[shelf]]): 250 ms, 50 ms only with the left button down within 300 dip; stops after 3 s near without a press, or at once away |
| `TIMER_OUTSIDE` (24) | 40 ms while a panel is kept open (can be hours) | 40 ms with input in the last second, else 120 ms (`outside_poll_ms`, `GetLastInputInfo`); a `WM_MOUSEMOVE` speeds it up at once |
| `TIMER_BEHIND` (29) | 80 ms while "Di belakang jendela" is on | 80 ms near the top edge (150 dip) or while lowering is due, else 350 ms (`behind::poll_ms`) |
| `TIMER_POINTER` (33) | new, 2026-10-30 | one `GetCursorPos` for the notch peek and the cat's look, see below |

So an idle island, pointer elsewhere, runs none of them except
`TIMER_POINTER` (with stay-behind on: ~3 a second). Not measured again after
the change.

### `TIMER_POINTER` (the notch peek and the cat's look)

One repeating timer through `App::pace` / `SetCoalescableTimer`, handled like
`TIMER_BEHIND`. Paces (`pointer_watch::poll_ms(follow_on, peek_on,
near_top)`; near = the pointer within `behind::NEAR_TOP_DIP`, 150 dip, of the
island monitor's top edge):

| Cat follows (look) | Peek | near | far |
|---|---|---|---|
| on | any | 33 ms | 250 ms |
| off | on | 80 ms | 350 ms (the same as `behind::POLL_MS` / `SLOW_MS`) |
| off | off | no timer | no timer |

It stops while fullscreen, while the session is locked or the display is off
(`screen_away`), and whenever neither feature is eligible: the peek needs
stay-behind off and a quiet compact view ([[notch-peek-eyes]]); the look needs
the cat on, motion Full (not Energy Saver) and the idle view
([[cat-looks-at-pointer]]). `App::sync_pointer_watch` starts, re-paces or
stops it, called from `sync_timers` (so every `refresh`), `sync_pointer_polls`
(lock, display), `follow_pointer_settings` (config adoption: stay_behind,
robot, robot_animation; Energy Saver goes through `check_battery`'s refresh;
fullscreen through the event's refresh). Each tick does one `GetCursorPos`
and feeds both. The window walk of the peek is not part of the poll's idle
cost: it runs only when the pointer first comes into the zone round the notch
and at most once a second while it stays. With stay-behind on and the island
lowered, the poll still runs (the look needs it when nothing covers the
island), but a window over the island stops the look from setting targets:
the tick adds at most one `WindowFromPoint` a second and starts no frames
([[cat-looks-at-pointer]]).

**The eyes' look (shrunk notch, v3.11).** While the notch is shrunk with
"Ikuti kursor" on (cat or not), `follow_on(look_eligible, eyes_look_eligible)`
is true, so the poll runs at the follow pace (33 ms near the top edge, 250 ms
far) instead of the peek-only 80 / 350 ms. The eyes ease on the `FrameClock`
only while they move (`eyes_look.moving()` in `animating_at`); with Saver,
Off, Energy Saver or Windows animations off they jump with one draw and no
clock. Nothing changes while the notch is not shrunk: an idle island costs
what it did.

**The typing cat's glance (2026-11-01)** adds nothing to the poll: it is
`look_eligible` through `pointer_watch::look_on` (`Expr::follows`), so the poll
runs at the follow pace exactly when the looking cat's would; the stillness
check (`GlanceRest`, 1.2 s) rides the same readings. A pointer at rest sets the
target back to (0, 0) once and then costs the poll's `GetCursorPos` only.

**The look's ease runs on the `FrameClock`**, not on this timer: a poll that
moves the look target by 0.02 or more starts the clock (`ensure_clock`),
`animating_at` counts `look.moving()`, and the clock stops once the ease has
arrived. A small pointer movement costs no frame. The shrink to the eyes is a
morph on the same clock (`EYES_MS`, 320 ms, ease-in-out; or it settles at once
with Motion not Full); while the notch is shrunk the equalizer timer is off
and the blink timer runs the eyes' own schedule instead of the cat's (above).
The fast-entry fix (2026-10-31) adds no timer and no
poll pace: the island's first `WM_MOUSEMOVE` while hover needs arming steps the
peek once (one `GetCursorPos`, at most one `UnderCache`-limited window read, so
at most once per approach, never at idle), and an over-the-notch reading reads
the windows only when the pointer just arrived from outside the zone.

**v3.8: the two toggles, and nothing else.** Settings → Kucing → Gerak has
"Ikuti kursor" (`cat_follow_pointer`) and "Mengecil jadi mata" (`notch_peek`),
both on by default. They are the only change to this poll: `look_eligible`
needs `pointer_watch::look_allowed(robot, cat_follow_pointer)` and
`peek_eligible` needs `peek_allowed(notch_peek, stay_behind)`, so with both
off, or neither eligible, `poll_ms` is 0 and **no `TIMER_POINTER` runs**
(`the_two_toggles_gate_the_poll` tests every combination). A change in
Settings re-syncs through `adopt_config` -> `follow_pointer_settings`, so the
poll starts, re-paces or stops at once and the look snaps back to the centre.
The cat colours and accessories ([[cat-accessories]]) add **no repeating
timer and no idle frame clock**: the painter builds its paths once per ear
state and the Kucing preview in Settings is a still picture (the chips only
repaint the window when clicked).

Gotcha: the System page and the clipboard were built in parallel runs and
both took ids 17 and 18; the merge moved the clipboard's to 20–22.
`app::tests::timer_ids_never_collide` now checks every id.

Related: [[bata-robot-expressions]], [[studio-media-progress-timer]], [[system-page]],
[[clipboard-history]],
[[lyric-notch-width]] (a long lyric line's one-shot scroll timer follows the same rule)

The notice ✕'s 120 ms fade ([[notice-close]]) rides the same `FrameClock`
(`close_fade.moving(now)` in `animating_at`): no new timer, and it is instant
under Energy Saver, Motion Off or Windows animations off.

Part of [[v3-studio-overview]]. Listed in [[index]].

## Desktop widgets (2026-11-04, [[widgets]])

Idle widgets cost nothing: no repeating timer, no frame thread, no redraw
without input or data.

- `TIMER_WIDGET_SAVE` (45): a one-shot armed 1.5 s after the last widget change
  (typing, a drag, a colour), replaced by each new change, flushed on
  `WM_DESTROY` / `WM_ENDSESSION`, cleared by any notes write.
- `TIMER_WIDGET_CARET` (46): a timer of the note window itself,
  `GetCaretBlinkTime`, alive only while that note has the keyboard, killed on
  blur or when the window goes.
- The header's hover fade (120 ms) and the gallery highlight (600 ms) run on
  the existing `FrameClock` only while they move (`widgets_moving` in
  `animating_at`, `NeedsClock` event), instant with Motion not Full, Energy
  Saver or Windows animations off.
- The fullscreen check rides the 1 s `TIMER_SYSTEM` tick: nothing unless a
  pinned note (or the Top layer) is shown and "Sembunyikan" is on, then one
  `is_fullscreen` per monitor holding such a note.
- Hover tracking is `TrackMouseEvent(TME_LEAVE)`, no timer; the cat peek is a
  still picture, no timer. One extra posted message type (`WM_APP + 71`) per
  burst of events, until drained.

The sticky notes' v2 ([[widgets]], 2026-11-04) adds one timer and three
FrameClock users, nothing else:

- `TIMER_STICKY_REMIND` (47): a one-shot on the island's window for the nearest
  unfired sticky note reminder (`remind::nearest`, wait clamped to the timer
  limits). Killed and re-armed on every reminder change (`widgets_sync`, which
  also runs at startup), on `WM_TIMECHANGE` and resume (`on_clock_jump`) and on
  each fire, where it is killed first. No polling; with no reminder the timer
  does not exist. A fire marks the due notes, raises them and puts a sticky
  island notice up, which then costs what any sticky notice costs.
- FrameClock, only while it moves (`WidgetHost::moving`, counted with the header
  and highlight fades): the reminder's 600 ms shake, the 0.8 s confetti of a
  finished checklist, and the 120 ms see-through fade of a note's window alpha
  (`present` only, no redraw). None of them starts when `animate` is false
  (Motion not Full, Energy Saver, Windows animations off); the fade is then
  instant.
- The cat's mood check (asleep after three days untouched) rides the existing
  1 s `TIMER_SYSTEM` tick (`WidgetHost::mood_tick`, next to the fullscreen
  check): one mood evaluation of the cat's note, a redraw of that note only when
  the mood changed. The cat looking at the pointer is computed on mouse moves
  over the note, quantised so it redraws only when the step changes.
- Idle notes still cost nothing: no redraw, no timer of their own.

Run B of the sticky notes ([[widgets]], 2026-11-04: stacks and tuck to the edge)
adds no timer and no thread, only two more FrameClock users in `WidgetHost::moving`:

- The tuck's slide (180 ms ease out and back in: `Win::slide`) and the **300 ms
  grace** between the pointer leaving a note that is out and its slide back in
  (`Win::grace`). The grace has no timer of its own: it is counted in `moving()`,
  so the frame clock runs for it, and `tick` ends it and starts the slide; nothing
  is drawn while it waits. Both start from a `NeedsClock` event like the other
  fades. The slide is instant with Motion not Full, Energy Saver or Windows
  animations off; the grace still waits its 300 ms on the clock. A tucked note
  at rest, a tab, and a stack at rest cost nothing: no redraw, no timer.
- The hover and leave ride the existing `TrackMouseEvent(TME_LEAVE)`; the stack's
  Shift highlight is read on mouse moves of a drag only (no polling), and the wheel
  flip is a message handler.
