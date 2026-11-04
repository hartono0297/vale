---
tags: [decision, notices, notifications, geometry, motion, hover, power, contrast, demo]
date: 2026-11-03
---

# The ✕ on a timed full notice

A small ✕ at the right end of a timed notice in its full form, so a
notification can be dropped without opening its app (most useful over a
fullscreen video). A click on the ✕ closes the notice and opens nothing.
Related: [[toast-while-busy]] (forms), [[notification-open]] (what the body
click does), [[notification-history]], [[clipboard-notice]],
[[cat-colours-and-surfaces]], [[notch-size-and-fill]], [[open-mode]],
[[bata-idle-power-budget]] (the fade rides the frame clock).

## Which notices get it (`state::notice_closable`, pure)

Every kind except: `Screenshot`, `ClipPreview`, `Arranged` (they have buttons of
their own: Salin / Simpan / Hapus, Riwayat, Batalkan / Simpan) and the sticky
kinds `Reminder`, `ClaudeWaiting`, `MoleFailed`, `UpdateAvailable` and
`StickyReminder` (a sticky note's reminder, with its own chips; they stay
until acted on, so a ✕ that drops them unseen would lose what they say). The
sticky ones stay excluded even when one is shown briefly as `current` in
fullscreen. What is left: `App` (real toasts and the island's own App
notices), `ClaudeDone`, `BatteryLow`, `Pomodoro`, `Greeting`, `SystemLow`,
`CleanDone`, `MoleDone`, `Copied`, `ShotSaved { .. }` (with a path its body click is `Effect::RevealFile`; the ✕ is still effect-free), `ShotDeleted`,
`DevicePlugged`, `DeviceSafe`, `UpdateProgress`, `UpdateFailed`, `UpdateInfo`.
`notice_kind_closable(&NoticeKind)` is the same by kind (the renderer's
`notice_text_fits` has only the kind).

`IslandState::notice_close_shown()`: `current` is Some, `notice_closable`
holds for it and `!toast_tucked()`. True for the Full form and the peek of a
toast, false for Strip, Slot, and for a sticky notice shown from `base_view`
with `current` None.

## The reducer rule (`Hit::NoticeClose`)

Acts only while `notice_close_shown()`. It sets `current = None` directly: it
does **not** call `dismiss_visible_notice` and not `toast_left`, so it emits no
Effect at all (no `OpenTarget`, no `OpenClaudeSession`, no Pomodoro start, no
page change) and sets no unread dot ("seen": the notice was shown full, the
`toast_full` / `toast_seen` flags are reset by hand). It clears
`toast_pointer`. The final `advance` of `apply` shows the next queued notice
as usual, with its whole time; in fullscreen with an empty queue the view is
`View::Hidden`. `linger_trigger` gives `LingerTrigger::No` for the click (no
panel is behind a closed notice). `Hit::Body`, `Hit::Strip`, `Hit::ToastSlot`
and every other path are unchanged.

## Hover: `hover_blocked` and the HoverTracking quiet flag

- Reducer: when `hover && visit_on_notice` (the pointer's visit began on the
  notice) the ✕ sets `hover_blocked = true`, so the panel does not open under
  the pointer for the rest of this visit (a notice without a button, like
  ClaudeDone, would otherwise hand the island to hover the moment it is gone).
  A panel that was open before the notice came (the visit began on it) or a
  latched one shows again as today. A wheel notch or a dock click spends
  `hover_blocked` on opening the panel as it does over a sticky reminder
  (a deliberate act, so it is allowed).
- `App`: the ✕ can be clicked before the 120 ms `WM_MOUSEHOVER`.
  `HoverTracking::close_quiet()` (set in `on_release` when the released hit is
  `Hit::NoticeClose`, before the Click is applied) makes `on_hover()` return
  `None` and not set `hovered`; `on_leave()` clears it. A visit that closed a
  notice never sends `Hover(true)`, so the dwell cannot open the panel. If
  `Hover(true)` was already sent, the reducer rule above covers it and
  `on_leave` still sends `Hover(false)`.

## Geometry (`ui/geometry.rs`, pure, tested)

`NOTICE_CLOSE_HIT` 24 (hit square), `NOTICE_CLOSE_INSET` 8 (shape's right edge
to the square's right edge), `NOTICE_CLOSE_DISC` 20 (painted circle),
`NOTICE_CLOSE_GLYPH` 9 (`Icon::X`), `NOTICE_CLOSE_GAP` 6 (text or chip to the
square). `notice_close_rect(shape)` is the 24 x 24 square, vertically centred.
`notice_layout_with` / `toast_notice_layout_with` / `notice_actions_layout_with`
take `close`: the right edge of the text and the chip block (`edge`, normally
shape right - 20) becomes `rect.x - 6`, which is 18 dip earlier. `notice_layout`,
`toast_notice_layout` and `notice_actions_layout` stay as wrappers with
`close = false`. Tests: all four corners of the rect are inside `shape_contains`
for Pill and Notch, robot on and off, several `NotchSize` (notices do not follow
the notch size); at least 24 x 24; title, body, eyebrow and chip end at or
before `rect.x - 6`; `HitRegions::hit` at the centre is `Hit::NoticeClose`,
at the text `Hit::Body`.

**Decision: the reservation is permanent for a closable notice**, whether or
not the ✕ is shown. Nothing moves when the pointer arrives; the text simply
ends 18 dip earlier than before. `Renderer::notice_text_fits` uses the same
room (so the cleaning notices pick a shorter body that fits, instead of an
ellipsis); the existing cleaning-notice fit tests still pass with it.

## Fade (`ui/motion.rs`, no new timer)

`CLOSE_FADE_MS` 120. `close_fade_ms(windows_animations, motion, energy_saver)`
is 0 (instant) when Windows animations are off, the robot's `Motion` is Off or
Energy Saver is on, else `CLOSE_FADE_MS`. `CloseFade { from, to, start, ms }`:
`alpha(now)` (ease-out cubic, opacity only, nothing moves), `retarget(to, now,
ms)` (continues from the alpha shown at `now`), `moving(now)`. In `App`:
the target is 1 while `!hover.needs_arming()` (the pointer is on the island
this visit, the existing tracking) and `notice_close_shown()` and the notch is
not shrunk to the eyes. `sync_close_fade` runs in `refresh` and again in
`begin_hover` after the tracking is armed (`refresh` runs before it). It starts
the `FrameClock` only when the fade moves, and `close_fade.moving(now)` is in
`animating_at`, so the clock runs only while it fades. `close_hot` (the pointer
is on the ✕, `hit_at == Some(Hit::NoticeClose)`) is tracked in `on_mouse_move`,
cleared in `on_mouse_leave`, and redraws only on a change.

## Render (`render.rs::paint_notice`)

A closable notice always lays out with `close = true`. With `frame.notice_close`
Some (alpha > 0): a 20 dip disc in the middle of the 24 square, `p.raised`
(`p.raised_hi` while hot), `Icon::X` at 9 dip in `p.text` inside `solid_ink`,
both at the fade alpha; then `notice_close_regions` is **extended** into
`self.regions.regions` (the reminder arm assigns, so extend). The region is
registered only when the ✕ is painted, so drawing and hit testing cannot drift.
`paint_shot`, `paint_arranged`, `paint_update_offer`, the strip and the slot
draw none.

## Contrast (measured, `theme::tests`)

`p.text` (#F4EDE4) on the disc, for every island surface (Auto resolves to one
of them), `raised` / `raised_hi`: Espresso 14.30 / 11.19, Graphite 12.78 / 9.47,
Sea 12.03 / 8.68, Night 12.65 / 9.19, Plum 11.05 / 7.91, Terminal 12.57 / 9.11.
All at least 4.5:1 (a test asserts it). The island has no light palette, so
"dark and light" means every island surface plus the see-through fill
([[notch-size-and-fill]]): the disc is its own opaque fill, so the glyph keeps
that contrast whatever shows through the island.

## Demo (debug builds)

Scenes in `--demo` (made-up data): `notice-close` (the chat toast with its
logo, ✕ shown), `notice-close-hot`, `notice-close-claude` (a Vale notice with
Bata), `notice-close-chip` (Ready Pomodoro chip plus ✕), `notice-close-reminder`
(sticky reminder: no ✕ even with the pointer over it). `DI_DEMO_CLOSE=0.5`
freezes the fade at that alpha. `--demo-notice-close`: the island window with a
real `IslandState` fed made-up notices (chat toast with `toast: Some`,
ClaudeDone with a made-up session id, CleanDone, a sticky Reminder), refilled
when empty. A thread-local queue carries WM_MOUSEMOVE / WM_MOUSELEAVE
(`TrackMouseEvent`) / WM_LBUTTONUP from its window procedure; a click applies
`Event::Click(hit)` from `renderer.hit_test`. Effects are never executed: they
are printed to stderr as `effects: [...]` (a body click prints `OpenTarget`, on the saved screenshot notice `RevealFile(..)`; a ✕
click prints `[]`). `DI_DEMO_FULLSCREEN=1` first applies `Event::Fullscreen(true)`.
No clipboard, notifications, media keys, config writes or history. A sticky
reminder in the cycle holds it until its Selesai chip is clicked (it prints
`ReminderDone`).

## Not verified live

Pure rules and tests only, plus a look at the five static scenes and a run of
`--demo-notice-close` on this machine (the ✕ appears under the pointer, a ✕
click printed `effects: []`). Not checked with a real toast over a real
fullscreen video, the `WM_MOUSEHOVER` race by hand (the quiet flag has unit
tests), the fade by eye on every surface, or a see-through notch with the ✕.

Listed in [[index]].
