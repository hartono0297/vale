---
tags: [decision, recording, notch, hover, open-mode]
date: 2026-11-08
---

# Mute from the closed REC notch

Each recorded source's level group on the closed REC view
(`paint_record_compact`) is a click region, `Hit::RecMuteCompact(source)`,
24 dip wide and the full notch height (`rec_mute_regions`). A click toggles that
source's mute (`Effect::RecMute`) in every open mode, opens nothing and
lingers nothing. Related: [[recording-controls]], [[open-mode]],
[[notice-close]], [[toast-while-busy]].

- **Hover without opening the panel.** `on_mouse_move` reports the group under
  the pointer (`Event::RecMuteHot`, only on a change, applied at once so the
  pill redraws: no timer). `panel_held` and `hover_stay_latches` require
  `rec_mute_hot` to be `None`, so a pointer resting on a group never opens or
  latches the panel; moving onto the rest of the notch reports `None` and the
  panel opens at once.
- **Gotcha: never clear the hot group just because the view changed.** A
  notice (a "Claude selesai" every few seconds on the owner's PC, a sticky
  "Claude butuh respon") replaces `RecordCompact` while the pointer is still
  on the group; clearing it there held the panel under the notice and showed
  it when the notice ended. It is cleared only on `Hover(false)`, the app's
  own reports, or when the groups really go (stop, pause, fullscreen), and
  then `hover_blocked` keeps the panel shut for the rest of that visit.
- **Gotcha: the leave order.** `on_mouse_leave` sends `Hover(false)` first and
  `RecMuteHot(None)` after; the other order is "the pointer moved onto the
  body" to the reducer and opened (or, in Hover & stay, latched) the panel.
- No regions while paused ("dijeda" replaces the meters).
