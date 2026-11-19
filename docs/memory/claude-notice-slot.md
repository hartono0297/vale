---
tags: [decision, notifications, notices, lyrics, claude, state]
date: 2026-11-19
---

# Claude notices tuck beside the lyric line

Report: a Claude hook notice covered the lyric line, so the song text could not
be read. [[toast-while-busy]] (Ringkas) already tucked a real Windows toast into
a logo slot beside a compact view; the two Claude notices were not part of it.

## Root cause

The tucking was gated on `is_real_toast` (`NoticeKind::App { toast: Some(_) }`).
`ClaudeDone` (timed, in `current`) was a full `View::Notice` for its 5 s.
`ClaudeWaiting` is sticky: `base_view_with` returned `View::Notice(sticky)`
ahead of the media, so the lyric stayed hidden until the session answered.

## The rule (`src/state.rs`)

- `is_tuckable(notice)`: a real toast, `ClaudeDone` or `ClaudeWaiting`. A Claude
  notice tucks only with `ToastStyle::Compact` and only over `MediaLyric` or
  `MediaCompact` (`toast_candidate`); over a panel, idle, REC, pomodoro, caption,
  fullscreen or with Penuh nothing changed. Strip, the unread dot, the in-place
  replacement and `countdown_admits` stay real-toast only (`is_real_toast`).
- `ClaudeDone` rides the toast path (form Slot, peek, click =
  `OpenClaudeSession`). A real toast arriving over a tucked `ClaudeDone` queues
  behind it (the replacement arm needs the current one to be a real toast).
- Sticky `ClaudeWaiting`: `compact_view()` is the media/idle tail of
  `base_view_with`; `sticky_slot_candidate()` (Compact, newest sticky is a
  `ClaudeWaiting`, `compact_view()` is a music view, no fullscreen/countdown),
  `sticky_peeking()` (pointer on the slot, `current` empty, panel shut),
  `sticky_tucked()`. `base_view_with` skips the sticky cover while tucked;
  `toast_slot()` falls back to it; `toast_peeking()` includes the sticky peek.
  It stays as the badge until the session shows activity (same clearing as
  before): no expiry, no dot. A real toast or `ClaudeDone` in `current` takes
  the slot for its time, the waiting badge comes back after.
- The badge is the letter tile "C" (source "Claude"); `App` needed no change
  (everything reads `toast_slot()` / `toast_is_peeking()`).
- `dismiss_visible_notice` resets `toast_pointer` when it takes `current`: a
  click on a toast over a waiting slot must not turn into a peek of the waiting
  notice (a second click would have opened its session).

## Known, accepted

- A hover that opens the panel over a tucked `ClaudeDone` turns it into the
  full notice over the panel, as before (no Strip form for Claude notices).
- A fullscreen brief copy of `ClaudeWaiting` latches Full; its click is
  `Hit::Body`, one `OpenClaudeSession`, and clears the sticky twin.
- A sticky Reminder / MoleFailed under a newer `ClaudeWaiting` stays hidden
  (it was hidden under the cover before as well).
- Two waiting sessions: a click on the slot opens the newest and the other one
  peeks full under the pointer until it moves (not fixed; rare).

Demo: `notice-busy-lyric-claude` (`slot_claude` flag of the demo `Scene`). Not
checked with a real hook on a real island; only the reducer tests and the demo
scene.

Related: [[toast-while-busy]], [[claude-notice-open]], [[lyric-notch-width]],
[[notice-close]]. Listed in [[index]].
