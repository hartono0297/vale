---
tags: [feature, widgets, sticky-notes, win32, power]
date: 2026-11-04
---

# Widgets, phase 1: the host and the sticky notes (Catatan tempel)

Plan: `docs/design/rencana-widget.html`. Phase 1 is a generic desktop widget
host plus one widget, the sticky note. A sticky note is an ordinary note
(`notes.json`, [[panel-pages]]) with an extra `sticky` part (paper, text size,
pin, roll, place, monitor, last touch). One data set: the Notes page shows and
edits the same note, "Lepas dari desktop" only removes the sticky part, and an
empty one is deleted then.

## Code map (`src/widgets/`)

- Pure and tested: `prefs.rs` (`WidgetPrefs`: show, layer Desktop/Top,
  fullscreen Hide/Keep, lock, cat_peek; config key `widgets`, island-owned,
  lenient per field), `sticky.rs` (`Sticky`, `Paper`, `TextSize`, tones and
  contrast, `MAX_STICKY` 20, footer and rolled labels), `checklist.rs`,
  `snap.rs`, `place.rs`, `apply.rs` (events applied to the store, where a new
  note goes).
- Win32: `host.rs` (windows, messages, drag, menu), `paint.rs` (drawing and
  the geometry the pointer shares), `edit.rs` (hidden EDIT), `pop.rs` (colour
  popover), `demo.rs`. `src/app_widgets.rs` is the island's side.
- `arrange_overlay::Canvas` was generalised: `with_gfx(Rc<Gfx>, size)` and
  `parts()`, so every note window shares one Direct2D factory (the icons, the
  cat geometry and the fonts hang on it).

## Data in notes.json

`sticky` is skipped when `None` (an old file loads unchanged and saves back
without the key). It is read leniently (`sticky::lenient`): not an object is
`None`, a wrong field is its default, the rect is clamped to 160x120 .. 640x800
dip; a bad sticky never moves the file to `.bak`. `NotesStore`: `set_text`
(a desktop note keeps empty text; a plain one follows `upsert`), `set_sticky`,
`create_sticky`, `sticky_count`, cap 20 (the Notes editor's empty `upsert`
still deletes). `note_time_label` moved to `notes.rs` and is shared by the
Notes page and the footer.

## Layers, Win+D

One `ValeWidget` layered popup per note (`WS_EX_TOOLWINDOW`: no taskbar button,
not in Alt+Tab). Desktop layer: `HWND_BOTTOM` with the shell window (Progman)
as owner, the way Rainmeter keeps skins on the desktop; `WM_WINDOWPOSCHANGING`
forces `HWND_BOTTOM` again (mode in `GWLP_USERDATA`, so the procedure needs no
borrow), so a click that activates a note never raises it. Pinned notes and the
"Di atas jendela" layer are topmost. `TaskbarCreated` re-applies owner and
order. **Win+D is not verified**: the owner trick is the documented Rainmeter
approach; if Windows hides the notes anyway on this build, that is accepted.
Susun never sees a note (`is_alt_tab_style` is false for a tool window, and it
skips its own process; tested). `ValeWidget` is not in capture `KEPT_OUT`
(notes belong in screenshots).

## Drawing and the transparent margin

The window is the note plus a margin: 8 dip left, right and bottom (soft
shadow), 8 + 30 dip on top (the cat). Alpha 0 is click-through, so the margin
costs nothing. The note is drawn in dip under a scale transform; the font is
Vale's Font setting (`Face::for_role_in`). Per-line text layouts (title
semibold, body regular, items indented for a drawn box; done items struck
through and dimmed), caret and selection from `HitTestTextPosition` /
`HitTestTextRange`, click to index from `HitTestPoint`. Paper tones are in
`sticky::tones`; every text tone is >= 4.5:1 and the accent >= 3:1 on every
paper, on all six surfaces (test). Gelap is the island's resolved surface
(Terminal included) with the island text colour and a 3 dip accent strip.

## Checklist rules (`checklist.rs`)

Lines `- [ ] ` / `- [x] ` (`- [X] ` read too), first line is the title. Indices
are UTF-16 units of the `\n` text; `edit_index` / `model_index` convert to the
EDIT's `\r\n`. A space after a line-start `-` makes a box, Enter on an item
continues (an empty one ends the list), Backspace at the item's text start
takes the whole marker, the caret never rests inside a marker (`snap_caret`),
the header button adds or removes the marker on the caret's line. Edits go
through `EM_REPLACESEL` with undo allowed.

## Hits, carets and surrogate pairs (2026-11-04)

Every index that leaves the layout is a UTF-16 unit index of the `\n` text, and
a click, a drag-selection and Up/Down all get theirs from
`BodyLayout::index_at` (`left_down`, `mouse_move`, `move_vertical`), which
feeds `edit::set_selection` through `checklist::edit_index`.

- **A trailing hit is after the whole cluster.** `HitTestPoint` answers with
  `DWRITE_HIT_TEST_METRICS` for the hit cluster (`textPosition` its start,
  `length` its length in UTF-16 units) plus `isTrailingHit` (windows crate
  0.62.2; the Microsoft docs confirm the cluster semantics). The caret is
  `textPosition + length` for a trailing hit, not `+ 1`: that put the caret
  between the two halves of an emoji, and typing or deleting there split the
  pair, `from_utf16_lossy` read the lone half back as U+FFFD, and it went into
  `notes.json` and onto the Notes page. Pure helper `paint::hit_index`.
- **A combining mark can be a cluster of its own.** With the shipped font
  DirectWrite answers `textPosition 3, length 1` for the `e` of "cafe" plus
  U+0301, so the trailing hit is 4, between the letter and the accent (found by
  the real-layout test). `paint::settle_index` moves the index forward past
  any surrogate half and any common combining mark, variation selector or
  joiner (`is_extend`: U+0300-036F, 1AB0-1AFF, 1DC0-1DFF, 200D, 20D0-20FF,
  FE00-FE0F, FE20-FE2F). Other marks stay legal indices: nothing is lost,
  both sides are whole code points. `BodyLine` keeps its `content` units for
  it.
- **Last guard.** `edit::snap_to_boundary` (pure) moves an index that is
  inside a surrogate pair back to before the pair. `set_selection` and `apply`
  (every `EM_SETSEL` the note sends, apart from Ctrl+A's 0..-1) pass both ends
  through it. `edit::selection` only reads what the EDIT holds and needs no
  guard.
- Tests: real DirectWrite layouts in `paint.rs` (an emoji line and a
  combining-mark line; a sweep of x never giving an index inside a pair or
  between letter and mark), the pure helpers (`hit_index`, `settle_index`,
  `snap_to_boundary`), and an emoji round trip through `edit_index` /
  `model_index` / `apply` in `edit.rs`.

## Snap, placement, monitors

`snap.rs`: 8 dip grid, area edges, other notes (12 dip gap or edge alignment,
threshold 8, only neighbours pull for the gap), Alt = none, clamp inside the
placement area (`placement_area(work, reserved_bottom_dip())`, 0 today: the
Dock will feed it), min 160x120, max 640x800, guides returned and drawn as
accent dashes in the dragged note and the one it lines up with. New note: the
first free grid slot nearest under the notch on the notch's monitor (240x208);
(+) in a header: right, else left, else below. `place.rs`: saved in dip relative
to the monitor's placement area with the device path; a missing monitor falls
back to the primary, clamped, **without** rewriting the save (it returns when
the monitor does); a user move saves the monitor it ended on. The clamp shrinks
the size too (a 640x800 note on a 1080p screen at 150 % is 693 dip tall), so
the host keeps `Win::view` (`Placement::view`: shown place and clamped size,
unrolled height) and uses it, never `sticky.rect`, for the window, the text
layout, hit-testing, the EDIT and the resize origin; only a resize writes the
size back. Hot-plug rides
the existing `TIMER_DISPLAYS` / `reposition` path ([[notch-monitor]]), plus
`WM_DPICHANGED` on the note itself; no polling.

## Fullscreen

On the existing 1 s `TIMER_SYSTEM` tick, only with `show` and "Sembunyikan"
and a pinned note or the Top layer: one `is_fullscreen(Some(a note hwnd))` per
monitor holding such a note. Desktop-layer unpinned notes are never touched;
nothing eligible means no call at all.

## Keyboard

A click on the text activates the note (`MA_ACTIVATE`; header, boxes, grip
return `MA_NOACTIVATE`), makes the hidden multi-line EDIT (a child, never
drawn, created on focus, destroyed on blur) and puts the caret by hit test.
Esc, deactivation or a click elsewhere ends it (posted, not done inside the
EDIT's own handler). Up and Down move by the drawn lines. The IME composition
window is placed at the drawn caret. The caret blinks on `TIMER_WIDGET_CARET`
(46), a timer of the note's own window, only while it has the keyboard. The
colour popover keeps the editing (the note gets the EDIT back when it closes).

## Timers and messages

`TIMER_WIDGET_SAVE` 45 (one-shot, 1.5 s after the last widget change, flushed
on `WM_DESTROY` / `WM_ENDSESSION`, cleared by any notes write) and 46. App
message `WM_APP_WIDGET = WM_APP + 71` (the event drain). Hotkey id 7 (6 is
the lyrics card's, merged on main; `WM_APP + 70` is the card's, 71 is ours). Idle cost: [[bata-idle-power-budget]].

## Ways to make a note

Settings -> Widget gallery (+), tray "Catatan tempel baru" (greyed at 20 with
the reason in the label), the seventh hotkey row (no default; a short island
notice at 20), the pin on a Notes card (`Hit::NoteSticky`, 22 dip, top-right:
accent when on the desktop), and the (+) in a note's header. Each makes an
empty Krem note, gives it the keyboard (except the pin, the note has text) and
turns `show` back on. Settings -> Widget: [[settings-structure]]. The hotkey:
[[hotkeys]].

## Assumptions

- No drag-out-of-the-notch in phase 1.
- The cat peek paints the coat only, no accessory (a hat would be cut by the
  note's edge); Happy while the checklist is all done, else Idle; on the note
  touched last (max `touched`), a still picture.
- Delete from the right-click menu is two clicks (the menu item reads "Klik
  lagi untuk hapus" on the next opening and the footer says it); it disarms on
  any other action.

## Demo

`--demo-widgets [reset]` (debug): its own notes file in
`%TEMP%\vale-widgets-demo\notes.json`, six seeded notes (all papers, a 2 of 4
checklist, an all-done one, a rolled one, a pinned Gelap one), live
everything; `DI_WIDGETS_LAYER=top`, `DI_WIDGETS_LOCK=1`, `DI_WIDGETS_PEEK=0`,
`DI_LANG`, `DI_FONT`, theme variables. It ends when its last note is gone.

Its message target (`ValeWidgetsDemoOwner`, only the event and timer target;
the notes' owner is still the shell window) is a **hidden top-level tool
window** (`WS_POPUP`, `WS_EX_TOOLWINDOW`, never shown: no taskbar button, no
Alt+Tab entry), not a message-only one. A message-only window never gets
broadcasts, so `WM_DISPLAYCHANGE` and `TaskbarCreated` now reach it and the
demo follows a resolution change and an Explorer restart. Checked on this PC:
`FindWindow` finds it from the top level (it does not find a message-only
one), it is invisible with ex-style 0x80, and the six note windows come up
beside it; `PostQuitMessage` when the last note is gone is unchanged. Not
checked: an actual display change or Explorer restart against the demo.

## Verified and not verified

Verified: unit tests (all the pure modules, store, settings layout, tray,
hotkey, timer ids), and the demo on this PC: notes drawn with checklist
strikethrough, snap spacing, the cat over the last touched note, ticks and
typed characters sent as window messages (events, footer time, cat move,
`notes.json` written and positions on the grid), the Settings page and its
gallery.
Not verified live: Win+D (above), dragging and resizing with a real mouse,
a real IME, the colour popover and the native menu with a mouse, hot-plug and
different DPI per monitor, fullscreen hiding over a real game, the Notes page's
pin with a mouse.

Run in the finish pass (2026-11-04, after merging main with the lyrics card):
`cargo test --locked` and the debug demo launch (window classes listed; the
demo owner is top-level and hidden). A click on an emoji or an accented letter
is covered by real-layout tests, not by a mouse. Settings -> Tombol pintas has
seven rows and scrolls at the minimum size (tested).
