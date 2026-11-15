---
tags: [spec, mascot, animation, input]
date: 2026-11-15
---

# A mascot story stops only on a click on the notch

## Why

Owner report (2026-11-15): "ketika gw geser mouse, animasinya berhenti". It
stops near the taskbar too. They want a running story to end only when they
click the notch, and handed the details to the builder ("u atur terbaiknya").

## Root cause

Two paths end a running story on mouse movement alone:

1. **The input guess** (`input_ends_play`, `src/app.rs`): a frame where the
   last-input tick moved but `GetCursorPos` did not is taken for a key. A slow
   mouse (sub-pixel moves under pointer ballistics) or one pushed against a
   screen edge reports input without moving the cursor a pixel, so it reads as
   a key anywhere on screen, the taskbar included.
2. **The pointer gate** (`gate::transient_ok`, `pointer_near`): "near" is
   `pointer_watch::near_top`, the top 150 dip of the monitor across its whole
   width, made for the poll's pace. Any pointer in that band (tabs, address
   bar) ends an automatic story.

The hover path adds a third in Hover mode: a dwell on the notch opens the
panel, and an open panel ends the story.

## Approaches

- **A, a click on the notch only (picked).** Nothing the pointer or keyboard
  does ends a story, except a click on the notch, which ends it and opens the
  panel as the click would in Click mode. Hover opens no panel during a story.
- B, a dwell on the notch also ends it. Closer to Hover mode, but a pointer
  passing slowly over the notch ends it, which is the complaint.
- C, the click only ends the story, a second click or hover opens the panel.
  Two steps for no gain.

## Behaviour

While a story runs (automatic or "Mainkan maskot"):

- Mouse movement anywhere, keys, hotkeys that open nothing, the wheel, and
  clicks outside the notch do not end it. Clicks outside still pass through
  (`WS_EX_TRANSPARENT`, unchanged).
- A dwell on the notch opens no panel in any open mode; the hover is not
  spent, so a dwell after the story ends opens it as usual.
- The notch does not shrink to the eyes (notch peek) while a story runs.
- A left click on the notch ends the story (the 200 ms wind-down) and opens
  the panel in every open mode. A right click on the notch shows its menu as
  now and ends the story too.
- Unchanged: every permanent gate still ends it (fullscreen, a panel or notice
  coming up by any other path, recording, lock or display off, Energy Saver,
  motion or Windows animations off, the mascot or island shape changing, the
  clock jumping, a clock story with the clock turned off, the typing face).

Not changed: when a story may start (`may_start`, the presence window, the
pointer and eyes gates at start, cooldowns, presets).

## Done

Unit tests for the new rules (no input path ends a play; the gates that end a
running play no longer include the pointer, the eyes or the menu for an
automatic play; a click on the notch ends it), `cargo test --locked` green,
and a live check with `--demo` if a story can be started there.

## Left to the builder

Where the hover suppression and the click land in `app.rs`, how the click
opens the panel in Hover modes (an `open_by_click`-like path or a reducer
event), and whether `play_input` / `tick_to_keep` go entirely.
