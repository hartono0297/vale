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

- **Full latch.** `settle_toast` (the very end of every `apply`, after the
  final `advance` *and* the hover latch, the linger arming and `end_linger`:
  an editor closing with the pointer away passes through Idle before the
  linger reopens the panel, and judging there latched a strip Full) sets
  `toast_full` when the toast is presented Full by context. Once
  full it stays full until it ends, whatever comes under it (a panel that
  closes mid-toast shows the toast whole with its remaining time; music that
  ends under a slot does too). Context changes never restart the 5 s.
- **Replacement.** A new real toast over a tucked one (Strip or Slot) **with
  an empty queue** replaces it in place with expiry `now + NOTICE_DURATION`
  and `toast_pointer` cleared; the replaced one, if unseen, sets the dot.
  Behind a Full toast, a peeked one, any other notice, or while anything waits
  in the queue it queues as always. Without the empty-queue guard a run of
  toasts under 5 s apart restarted the time again and again and held back a
  queued screenshot, Disalin, Pomodoro or update notice for the whole run, and
  a newer toast could show before an older queued one.
- **Peek.** `Event::ToastPointer(true)` while the form would be Slot: the view
  is the full toast (`toast_peeking()`), **not latched**: when the pointer
  leaves, the form is Slot again (or follows the context). A peek blocks hover
  (`notice_blocks_hover`), so hover never opens the panel under the full toast.
- **Hold.** `notice_held` also holds for `toast_pointer` while the form is Strip
  or peeking. `toast_hold_moved` ends the hold when the spot the pointer held
  it on (`toast_held_spot()`: Strip, or Slot while peeking) changed: it drops
  `toast_pointer` (a panel shut by a key with the pointer on the strip
  otherwise turned straight into a peek with no pointer on the slot) and sets
  expiry = max(expiry, now + `SHOT_LEAVE_GRACE`). `apply` runs it after the
  linger update, after the event (both **before** an `advance`: the second
  `advance` judged a strip closed by a key as unheld Full and dropped a toast
  whose 5 s had run out at once, unseen, setting the dot) and in
  `settle_toast` (the latch / linger end). On `ToastPointer(false)` (or
  `Hover(false)`, which clears `toast_pointer` too) after holding, the same
  grace, like a notice with an action.
- **Clicks.** `Hit::Strip` while the form is Strip calls
  `dismiss_visible_notice` (the panel stays open; with a sticky strip it still
  dismisses the sticky one). `Hit::ToastSlot` while a slot or peek shows
  does the same; the peeked full toast is `Hit::Body` as always. A click counts
  as seen. What a click on a toast *does* is [[notification-open]]: since the two
  landed together, `dismiss_visible_notice` emits `Effect::OpenTarget` for
  every toast (strip, slot and peek clicks too, also one without an app id).
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
re-planned on the toggle exactly as on a width change (`refresh` sets the flag
after `sync_lyrics`, whose `LyricsReady` may change the view, and before
`sync_lyric_line`, which ends in `sync_lyric_scroll`). A line that hit the cap
gives the logo's 28 dip of its box up (it then scrolls and fades as ever).
`Frame::toast_slot_w` is `App::slot_share` (`SlotShare { from, to }`), set at
each retarget from the share drawn then and the target's (28 with the slot, 0
without; hiding keeps it as it is), eased with the shape's curve. It is **not**
read off the shape's width: that also changes with a longer lyric line or
MediaCompact to MediaLyric, and the logo blinked out and `Hit::ToastSlot`
dropped for the morph. A slot at both ends stays whole; only one that comes or
goes eases. A lyric line at its cap keeps the notch's width when the slot
toggles, so `slot_share.to` changing is a retarget of its own (a resize morph).
After a peek the logo eases back in with the shrink (the Notice drew none).
`App` keeps the slot's notice (`slot_keep`) until the island is back so the
logo fades out with the shrink. The morph uses the existing rules (the same
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
  the toast's 5 s (bounded: a toast never replaces one while the queue holds
  anything), the same timing as before.
- The peek's grace is `SHOT_LEAVE_GRACE` (2 s) after the pointer leaves.
- A toast that arrives in fullscreen is Full by context and counts as seen
  (nothing is shown in a game; as before it simply passes).
- A peek only starts when the full toast (360 wide, less
  `TOAST_PEEK_INSET` 8 dip a side) lies under the **pointer itself**
  (`geometry::toast_peek_reaches`, from `App::toast_slot_peekable`). The
  `Hit::ToastSlot` region runs from `logo.x - 4` to the shape's right end, so
  a compact or lyric shape wider than the toast has a band there with no peek
  (a click on it still works). Asking only whether the logo's centre lay in the
  toast left that band peeking with the pointer off the toast, and the notch
  flapped between the slot and the toast under a still pointer.
- The Settings texts are shorter than first asked so they fit the row in both
  languages ("Notifikasi saat notch dipakai" / "Notifications on a busy notch",
  note "Ringkas: strip atau logo kecil" / "Compact: a strip or a small logo").

## Merge with main (2026-11-02)

Merged after the hotkeys section and the typing cat landed on main
([[hotkeys]], [[cat-types-while-working]]). Both sides were kept; the one
thing dropped is the branch's PrtSc toggle in Settings (`ShotPrintScreen`
Hit), because main turned PrtSc into a hotkey row. The Notifikasi card keeps
the Ringkas/Penuh row first and the history rows one row lower. The slot never
applies to `ShapeKind::Idle`, so the typing cat's `idle_shape_typing` and the
slot arm do not meet; the demo `SCENES` list holds both sides' scenes (155).
`app_notif.rs` (main's start-logo fallback) is untouched.

## Not verified live

Only the pure rules (reducer, geometry, motion) have tests, plus a `--demo`
look at `notice-busy-panel`, `-lyric`, `-media`, `-pomodoro`, `-hover` and
`notice-unread-dot` / `-idle` (made-up data and logo). Not verified with a real
toast: Chrome / WhatsApp over an open panel page, lyrics, music without lyrics
and an idle notch; hovering the logo; the dot clearing (hover, Notifikasi page);
Settings switched to Penuh and back; the strip's motion and the slot's morph by
eye; the hover over a slot on a real island (the window is a layered one, so a
pointer off the drawn pixels sends no moves).

The ✕ ([[notice-close]]) shows on the Full form and on the peek of a toast,
never on the Strip or the Slot; it closes without an effect and without the dot.

Related: [[hotkeys]], [[cat-types-while-working]], [[open-mode]], [[notification-history]], [[lyric-notch-width]],
[[compact-padding]], [[notch-peek-eyes]], [[panel-linger-and-motion]],
[[bata-idle-power-budget]]. Listed in [[index]].
