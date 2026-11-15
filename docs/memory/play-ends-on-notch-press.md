---
tags: [decision, gotcha, mascot, animation, input]
date: 2026-11-15
---

# A mascot story ends only on a press on the notch

Owner report (2026-11-15): moving the mouse stopped the mascot story, near
the taskbar too. Two causes, both gone:

- **The input guess.** A frame where `GetLastInputInfo`'s tick moved but
  `GetCursorPos` did not was taken for a key. A slow mouse (sub-pixel moves
  under pointer ballistics) or one pushed against a screen edge sends input
  without moving the cursor a pixel, so it read as a key anywhere on screen.
  Gotcha: never tell keys from mouse moves by "input without a cursor move".
- **`pointer_near` as a stop gate.** It is `pointer_watch::near_top`, the top
  150 dip across the whole monitor (made for the poll's pace), not "near the
  notch". It now binds only the start of a story (`gate::transient_ok`), like
  the eyes and the menu; `must_stop` reads only the permanent gates.

What ends a story now: a press on the idle island (`play_press`): left in
Hover / Hover & tetap opens the panel by delivering the hover the story held
back (`EndAndHover`, the press is spent so its release is no click on the
panel); left in Klik goes through the normal click; right shows the menu;
middle ends it too. While a story runs a hover dwell opens nothing
(`hover_held`, disarmed so the next dwell after the story works) and the
notch never shrinks to the eyes (`peek_eligible`); "Mainkan maskot" restores
a shrunk notch first. Spec `docs/superpowers/specs/2026-11-15-maskot-berhenti-klik-notch-design.md`.

Not verified live: the owner checks it by hand (see [[mascot-play]]).
