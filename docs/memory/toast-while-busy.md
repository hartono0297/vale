---
tags: [decision, notifications, notices, notch, panel, geometry, motion, power]
date: 2026-11-01
---

# Ringkas: a Windows toast stops covering a busy notch

Setting **"Notifikasi saat notch dipakai"** (Settings -> Fitur -> Notifikasi,
first row, `config.notifications.toast_style`, `ToastStyle { Compact, Full }`):
**Ringkas** (Compact, the default) tucks a real Windows toast away while the
notch is in use; **Penuh** (Full) is the full takeover every toast was before.
Any other value in the file reads Compact (lenient reader, like `shot_tool`);
the field is the island's own (`merge_owned` keeps `notifications`), applied
live from Settings, at startup and from a hand edit (`adopt_config`) through
`Event::ToastStyle`. Only a notice made by `sources::notifications::to_notice`
(`NoticeKind::App { toast: Some(_) }`, `state::is_real_toast`) is touched.
Clipboard preview, Disalin, reminders, Claude, pomodoro, update, screenshot,
devices, greeting and `App { toast: None }` notices are unchanged. Background:
[[notification-history]] (the toast, its logo, the history).

## The form rule (`state.rs`, pure)

The toast stays in `current`, so the queue, `advance`, the 5 s timing and the
click path (`dismiss_visible_notice`) are exactly as before; only the *form*
is derived from context (`toast_form()`):

| Under it (`base_view()`, `view()` with `current` ignored) | Form | `view()` |
|---|---|---|
| Idle, Hidden / fullscreen, a sticky notice | Full | `Notice(toast)` |
| `Panel(_)` | Strip | the panel |
| MediaLyric, MediaCompact, PomodoroCompact | Slot | that view |

`toast_tucked()` = Compact style, a real toast, not latched full, not peeking,
base view in the table's last two rows. `notice_covers()` (current is Some and
not tucked) replaces `current.is_some()` in every hover rule (`hover_stay_latches`,
the Hover `visit_on_notice`, the Wheel / PageFlick / page-dock sticky peeks,
the Click-mode `Hit::Body` arm, `notice_blocks_hover`): with a tucked toast and
no peek the reducer behaves as if there were no `current`.

- **Full latch.** `settle_toast` (end of every `apply`, after the final
  `advance`) sets `toast_full` when the toast is presented Full by context. Once
  full it stays full until it ends, whatever comes under it (a panel that
  closes mid-toast shows the toast whole with its remaining time; music that
  ends under a slot does too). Context changes never restart the 5 s.
- **Replacement.** A new real toast over a tucked one (Strip or Slot) replaces
  it in place with expiry `now + NOTICE_DURATION`; the replaced one, if unseen,
  sets the dot. Behind a Full toast, a peeked one or any other notice it queues
  as always.
- **Peek.** `Event::ToastPointer(true)` while the form would be Slot: the view
  is the full toast (`toast_peeking()`), **not latched**: when the pointer
  leaves, the form is Slot again (or follows the context). A peek blocks hover
  (`notice_blocks_hover`), so hover never opens the panel under the full toast.
- **Hold.** `notice_held` also holds for `toast_pointer` while the form is Strip
  or peeking. On `ToastPointer(false)` (or `Hover(false)`, which clears
  `toast_pointer` too) after holding, expiry = max(expiry, now +
  `SHOT_LEAVE_GRACE`), like a notice with an action.
- **Clicks.** `Hit::Strip` while the form is Strip calls
  `dismiss_visible_notice` (the panel stays open; with a sticky strip it still
  dismisses the sticky one). `Hit::ToastSlot` while a slot or peek shows
  does the same; the peeked full toast is `Hit::Body` as always. A click counts
  as seen. What a click on a toast *does* (open the app) is the other session's
  area (`vale-notif-open`) and was not touched.
- **Strip vs sticky.** `waiting_strip()` and `shown_panel().strip` give the
  toast while the form is Strip, else `sticky.last()`: a sticky strip yields the
  slot for the toast's 5 s and comes back after.

## The unread dot

`toast_dot` is set when a real toast leaves `current` with `!toast_seen`
(expiry in `advance`, or replaced), only with the Compact style. "Seen" =
presented Full (context or peek) or the Notifikasi page showing. It clears on a
peek, at the end of `apply` while the view is `Panel(Page::Notifications)`
(which also marks the current toast seen), and when the style turns Full. A
toast that was Full from the start never sets it. Not persisted.
`toast_dot_shown()`: the dot, a view of Idle / MediaCompact / MediaLyric /
PomodoroCompact, no slot showing, not fullscreen; `App` also hides it while the
notch is shrunk to the eyes ([[notch-peek-eyes]]).

## Slot geometry (`ui/geometry.rs`, tested)

`TOAST_SLOT` 20 (logo), `TOAST_SLOT_GAP` 8, `TOAST_SLOT_EXTRA` 28, `TOAST_DOT` 6.
`with_toast_slot` widens a compact shape by 28, centred. `toast_slot_parts(shape,
base_w) -> (content, logo, alpha)`: `e = clamp(shape.w - base_w, 0, 28)`, the
content rect is the shape less `e` on the right (same x), the logo a 20 x 20
square ending `COMPACT_INSET` ([[compact-padding]]) from the shape's right,
centred vertically. Every compact layout (`compact_layout`, `lyric_layout`,
the robot variants, pomodoro) is computed on `content`, so nothing under the
logo moves at any frame and the last element ends 8 dip or more left of it. The
logo's alpha ramps over the last 8 dip of the growth (`e` 20 to 28), **not**
`e / 28`: with `e / 28` the logo would show over the bars at `e < 20` mid-morph
(a test walks the whole eased morph, overshoot included, both ways).

Lyric ([[lyric-notch-width]]): `lyric_slot_width(w, robot, min, max) =
min(w, lyric_cap - 28) + 28`, never above `lyric_cap`. `LyricFit` has a `slot`
flag: `width()` is the total, `content_w()` the line side, `box_w()` and the
scroll plan follow the content, and the one-shot `TIMER_LYRIC_SCROLL` is
re-planned on the toggle exactly as on a width change. A line that hit the cap
gives the logo's 28 dip of its box up (it then scrolls and fades as ever).
`Frame::toast_slot_w` = animated shape width less the target width without the
slot; `App` keeps the slot's notice (`slot_keep`) until the island is back so
the logo fades out with the shrink. The morph uses the existing rules (the same
view gives no content fade; the lyric uses the resize timing). The dot sits
8 dip in from the right at the vertical centre (`toast_dot`). The slot
registers `Hit::ToastSlot` (the logo and the island's right end) only once the
logo is fully shown.

`App::peek_eligible` is false while a slot shows (the logo must stay visible;
a notice ends the eyes today too). The pointer: `on_mouse_move` sends
`Event::ToastPointer(over)` only on change, `over` = on the strip while its
form is Strip, on the slot, or (peeking) anywhere on the island; a leave sends
false even before the hover dwell. The reducer is the authority: `refresh`
copies `toast_pointer` back, so a reset by a new toast or a leave never leaves
`App` thinking it already said true.

## The strip (`render_panel::paint_strip`, `ui/motion.rs`)

For a real toast the strip draws a 16 dip logo (or the letter tile of
`notif::tile`) and "Source · title" (the body when the title is empty),
ellipsised, `toast_strip_parts` (logo at +6, text from +28 to -10). The sticky
look is unchanged. When the strip appears or disappears while the panel stays
open and settled, `motion::strip_dir` (pure) decides `In` / `Out` and
`StripMotion` runs `REVEAL_MS` (160 ms, ease-out cubic): the content (laid out
under the strip throughout) eases by `STRIP_H` down (`In`) or up (`Out`) while
the strip fades 0 to 1 or 1 to 0; the page click regions wait for it (as for a
slide), the last frame is drawn settled. Reduced motion (`!animations`), a
panel opening or closing (it fades with the panel), another page at the same
time, and a toast and a sticky strip swapping are instant.

## Power

No new timer and no polling. The expiry rides `next_deadline` /
`TIMER_DEADLINE`; the hold simply removes it while the pointer is on the toast.
The slot's morph, the logo fade and the strip's motion are in `animating_at`
(the existing `FrameClock`). See [[bata-idle-power-budget]] and
[[panel-linger-and-motion]].

## Assumptions

- Other notices still queue **behind** a tucked toast: an urgent one waits up to
  the toast's 5 s, the same timing as today.
- The peek's grace is `SHOT_LEAVE_GRACE` (2 s) after the pointer leaves.
- A toast that arrives in fullscreen is Full by context and counts as seen
  (nothing is shown in a game; as before it simply passes).
- A peek only starts when the full toast (360 wide) would cover the logo's
  spot; a very wide lyric notch whose logo lies outside it gives no peek (a
  click on the logo still works), or the notch would flap between the two forms
  under a still pointer.
- The Settings texts are shorter than first asked so they fit the row in both
  languages ("Notifikasi saat notch dipakai" / "Notifications on a busy notch",
  note "Ringkas: strip atau logo kecil" / "Compact: a strip or a small logo").

## Not verified live

Only the pure rules (reducer, geometry, motion) have tests, plus a `--demo`
look at `notice-busy-panel`, `-lyric`, `-media`, `-pomodoro`, `-hover` and
`notice-unread-dot` / `-idle` (made-up data and logo). Not verified with a real
toast: Chrome / WhatsApp over an open panel page, lyrics, music without lyrics
and an idle notch; hovering the logo; the dot clearing (hover, Notifikasi page);
Settings switched to Penuh and back; the strip's motion and the slot's morph by
eye; the hover over a slot on a real island (the window is a layered one, so a
pointer off the drawn pixels sends no moves).

Related: [[open-mode]], [[notification-history]], [[lyric-notch-width]],
[[compact-padding]], [[notch-peek-eyes]], [[panel-linger-and-motion]],
[[bata-idle-power-budget]]. Listed in [[index]].
