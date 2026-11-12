---
tags: [decision, shelf, ole, drag-drop, v3.2, v3.3]
date: 2026-10-29
---

# File shelf (v3.2 Run 11)

`Page::Shelf`, right after Clipboard (`Page::ALL` is eleven pages; a saved
`panel_pages` without "Shelf" gets it back after Clipboard like any new page).

- **List** (`src/shelf.rs`, pure): paths only, never copies, in **memory
  only** (a restart empties it: decided, a persisted shelf would need stale
  handling across restarts and a privacy story). Max 12, deduped by path
  ignoring letter case and a trailing slash, extras ignored. `cards(probe)`
  gives name (`short_name`, cut to 14 chars keeping a short extension), icon
  kind (by extension, folder by probe) and `stale` (path gone), with the file
  system injected (`probe`) so tests never touch real files.
  Kinds: Folder, Image, Media (sound: mp3 wav flac m4a aac ogg, the music note),
  **Video** (mp4 m4v mov mkv avi webm wmv 3gp, the camcorder `Icon::Video`,
  added with the phone link so a received video is not drawn as a song), Code,
  Archive, Document. `shelf::icon` is the one `match` on the kind.
- **Drop** (`src/shelf_ole.rs`): `RegisterDragDrop` on the island window
  after `OleInitialize` (the thread was already `CoInitializeEx` STA from
  `app::run`; OLE just adds drag and drop on top). `DropTarget`
  (`#[implement(IDropTarget)]`, needs the `windows-core` crate as a direct
  dependency and the `Win32_System_SystemServices` feature) accepts only
  `CF_HDROP`. Effect answered: link if offered, else copy, **never move**
  (`choose_effect`, tested). It posts `Event::ShelfDrag(true/false)` and
  `Event::ShelfDrop(paths)` on the app channel + `WM_APP_EVENT`; nothing
  re-enters `with_app` from inside OLE.
- **Reducer**: `ShelfDrag(true)` opens the panel on the shelf and holds it
  (`shelf_held`, like `clip_held`: hover takes over, it ends with
  `ShelfDrag(false)`, a page change or a collapse); not in fullscreen. A
  drop makes `App` add the paths and apply `ShelfDropped` (v3.3: the drop
  latch, see the last section; the old 2.5 s hold is gone). Clicks (`Hit::ShelfCard/ShelfRemove/
  ShelfClear`) only count while the page is open; a card collapses the panel
  then opens its file (`Action::Shell`), a missing file is only marked.
- **Drag out**: a left press on a card whose file is there, then a move of
  more than 6 dip with the button down, calls `shelf_ole::drag_out` from
  `wnd_proc` outside the app borrow (`DoDragDrop` blocks and pumps messages).
  The data object is the shell's own for that item
  (`SHCreateItemFromParsingName` -> `BindToHandler(BHID_DataObject)`), offered
  for copy or link only; the card stays after the drop. The click that would
  have opened the card is dropped (`click.clear()`).
- Stale detection runs when the page opens (`sync_shelf`), after a drop or
  a change, and before a card is opened or dragged: no timer, no IO while
  the page is closed.
- Layout: four cards a row, up to three rows, an X on each card's corner
  (first in `regions`), "Kosongkan" beside the heading, an empty-shelf hint.
  Demo scene `shelf` (made-up names, one stale) and a `shelf` shot.

**Not verified live:** that OLE accepts drops on this layered, no-activate
window (and where: layered windows ignore fully transparent pixels, so only
the drawn island takes a drop), that the idle island is big enough to aim at,
that a drag hovering opens the panel in time, `DoDragDrop` out to Explorer and
other apps (and the effect each app picks), a mouse that stays still after a
drop (the drop latch is the fallback), and stale detection for network or
removable paths.

## v3.2.1: opening on a drag, and a tidier page (2026-10-29)

**Checked with a real OLE drag** (a throw-away WinForms window calling
`DoDragDrop` with a file list, steered by `SetCursorPos`, on the debug build
with its own APPDATA): `RegisterDragDrop` is `Ok`, a drag over the idle island
(190 x 34, clock showing) reaches `DragEnter` with `CF_HDROP`, the reducer
opens the panel on the shelf and a drop puts the card on it. So the chain
works when the pointer is *inside the drawn shape*.

**What was missing** (best-supported cause, the user's own drag is not
reproduced): the pointer was not on the island at all. A layered window with
per-pixel alpha only takes input on pixels it drew (alpha > 0), and the idle
Pill has an empty strip above it: `WindowFromPoint` at y = 2 is the Windows
search box, not Vale, so a drag aimed at "the top of the screen" never
entered. Fix: `HALO_ALPHA` (0.012, 8-bit alpha 3: invisible) is drawn on a
rectangle from y = 0 to 8 px under the island and 14 px beyond each side.
**Only while a drag can be about to start** (3.2.1 review fix): `halo.rs`
polls on `TIMER_HALO` (since the later review only after a trigger, see
below) for "left button down and the pointer within
100 dip of the island"; a drag from Explorer holds the button the whole way,
so the margin is there when the pointer arrives, and it is gone otherwise, so
it never blocks a click on the window beneath (a maximized window's tab
strip). A press that began elsewhere keeps its capture. On the margin itself
moves, dwell, wheel and right-click (no tray menu) are ignored
(`Under::Halo`); moving from the island onto it counts as leaving.

- OLE reports `Event::ShelfDragOver(bool)`; `App::on_shelf_drag` lights the
  drop zone (`ShelfState::dragging`, `Frame::shelf_drag`), passes `true` on as
  `ShelfDrag(true)` and, on leave, arms `TIMER_SHELF_HOLD` for 600 ms before
  `ShelfDrag(false)` (a drag that strays off the edge and back keeps the
  panel; re-entering cancels the timer).
- The reducer no longer ignores a drag during `fullscreen`: the island is
  hidden then, so no drag reaches it, and a wrong "fullscreen" reading (the
  shell's `QUNS_BUSY`) must not swallow a real one.
- Page: title with the count ("Rak berkas · 6"), a quiet "Kosongkan", cards
  with a 26 px icon over a name in up to two lines (`shelf::wrap_name`, one
  line with three rows), the X on a round chip, a "hilang" tag on a card whose
  file is gone, an empty state (dashed zone, download icon, hint), and the
  dashed accent drop zone with "Jatuhkan di sini" while a drag hovers. Demo
  scenes `shelf`, `shelf-empty`, `shelf-drop`, `shelf-drop-files`.

**Still not verified:** the halo with a real Explorer drag (the harness aimed
at y = 14 and at the idle island only; the y = 2 run was not completed), drops
on the halo itself, and drag-out. (The halo no longer sits over clicks: it
exists only with the button down.) `DI_SHELF_LOG` debugging was removed again.

## v3.2.1 review fixes (2026-10-29)

- A `Drop` that was accepting but reads no paths sends `ShelfDragOver(false)`,
  so the drop zone and the hold end (no drop follows).
- A card press is forgotten on `WM_MOUSELEAVE` and on a move whose `wparam`
  lacks `MK_LBUTTON` (the release may have happened off the island).
- Drag out: a thread-local `DRAGGING_OUT` (set around `DoDragDrop`) makes the
  island's own `DragEnter`/`Drop` ignore its own drag (no drop zone, no
  re-add). After `drag_out` returns, `shelf_after_drag_out` cancels the swipe
  (its release was swallowed) and runs the leave path if the pointer ended
  off the island. Still not verified live.

## v3.2.1 review: no idle halo poll (2026-10-29)

The 50 ms `TIMER_HALO` ran whenever the island was up: ~20 wake-ups a second
on an idle island, against [[bata-idle-power-budget]]. Now the poll only
starts from a cheap trigger and stops by itself (`halo.rs`):

- **Triggers:** a `WM_MOUSEMOVE` on the island window (the island or its
  margin) and an OLE `DragEnter` on it (`ShelfDragOver(true)` arms it; the
  panel opening pauses it, and it resumes when the panel closes, so a drag
  that strays above the island afterwards finds the margin).
- **Runs while** the left button is down (until the release) or the pointer
  stays within `NEAR_DIP` for `LINGER` (3 s) after the trigger
  (`keep_polling`); then it stops until the next trigger. Pace: `SLOW_MS`
  (250) and `FAST_MS` (50) only with the button down within 3 × `NEAR_DIP`
  (`poll_ms`), coalescable. Never while the session is locked, the display is
  off, fullscreen, or the island is lowered behind windows.
- **Trade-off, honestly:** OLE gives no message while a drag is still away
  from the island (the drag source holds the capture), so the poll cannot see
  a drag coming from far off. A drag aimed straight at the empty strip *above*
  an island it never touched finds no margin: it has to land on the drawn
  island itself (the idle island is 190 x 34 at the top, and that path,
  `DragEnter` on its pixels, is the one checked with a real OLE drag and works
  without the halo). The margin helps a drag that started near the island
  within a few seconds of the pointer passing over it, and a drag that
  touched the island, left and came back above it.
- The hit test now treats drawn pixels as the island: the Notch shoulders and
  a 1 px anti-aliased edge (`geometry::island_contains`, `AA_MARGIN`) give
  `Hit::Body`, and the margin has its own result (`Under::Halo`, from
  `HitRegions::halo`), so leave, dwell, wheel and right-click (no tray menu)
  skip only the margin, and a click on a shoulder never counts as outside.
- The logical left button is read (`left_button_down` checks
  `SM_SWAPBUTTON`), for the halo, the drag-out and the Mixer slider drag.

**Not verified live:** the trigger path with a real Explorer drag, and the
swapped-buttons case.

## v3.3: a drop opens the shelf and keeps it (2026-10-29)

**Cause of the collapse:** during the OLE drag the island gets no mouse
messages, so after the drop `hover` was false; the 2.5 s hold ended, or the
first pointer move began a visit whose leave closed the panel.

**Fix (`state.rs`, `Event::ShelfDropped`):** `App::on_shelf_drop` adds the
paths, kills a pending `TIMER_SHELF_HOLD` (it stays only for the 600 ms
drag-leave of `on_shelf_drag`; `SHELF_HOLD_MS` is gone) and applies
`ShelfDropped`. Not in fullscreen, the reducer sets page Shelf,
`collapsed = No`, `shelf_held = false` (the drag is over) and
**`latched = true`**: the panel then follows the ordinary latched rules (it
survives the pointer leaving, `TIMER_OUTSIDE` runs, `Event::Dismiss` closes
it, a card click collapses it, fullscreen or a mode change clear it).
`ShelfDrag(false)` only clears `shelf_held`, never the latch.

- **HoverStay and Click:** nothing more; "stays until a click outside" is
  their normal rule.
- **Hover mode, `shelf_drop: DropVisit { No, Here, Left }`:** the drop's own
  visit must not end the panel. `Here` is set by the drop; `Hover(false)` or
  `ShelfDropLeft` turn it into `Left`; the next `Hover(true)` with `Left`
  hands over (`latched = false`, `No`) so hover holds the panel and leaving
  closes it as usual. A page change by the wheel with hover on hands over
  too (without hover the dwell has not fired yet, so the latch is kept for
  it). Post-process invariant: `!latched` => `shelf_drop = No`.
- **`ShelfDropLeft`:** `HoverTracker::on_leave` returns `None` when the dwell
  never fired, so `App::on_mouse_leave` sends `ShelfDropLeft` itself when
  `state.shelf_drop_here()`. A stale tracker `Hover(false)` right after a drop
  is absorbed by the same `Here -> Left` step.
- **Gotcha:** in Hover mode a user who drops and walks away leaves the panel
  open until their next click anywhere. That is the requested "stays".

**Not verified live:** a real Explorer drop followed by moving away at once,
and the Hover-mode hand-over on the next visit.

Related: [[mixer]], [[open-mode]].

## v3.5: motion

Cards animate in, out and across: see [[shelf-motion]].

## v3.18: from a phone

The HP pill, a QR link and the cards tagged "dari HP": see [[shelf-phone]]. Since v3.19 the
whole shelf (every file card, no ticking) is shared with the phones that scanned the QR, and
a card taken off the shelf or "Kosongkan" stops being shared at once.

## 2026-11-12: drag out verified live, and shared with the Clipboard page

`shelf_ole::drag_out(path, allowed) -> Option<DROPEFFECT>` now takes the
allowed effects (the shelf passes `SHELF_DRAG_EFFECTS`, copy or link, as
before) and returns the effect (`None` = the drag could not start).
`past_threshold` and the after-drag steps (`drag_out_ended`) are shared with
the Clipboard page's image drag ([[clipboard-drag-out]]), which offers copy
only. That run checked the path live for the first time: a file from
`DoDragDrop` landed in an Explorer folder as a copy, and the Claude desktop
app (Chromium/Electron) took it as an attachment.
