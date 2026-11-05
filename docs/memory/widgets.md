---
tags: [feature, widgets, sticky-notes, win32, power]
date: 2026-11-05
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

`TIMER_WIDGET_SAVE` 45 on the island window (one-shot, 1.5 s after the last
widget change, flushed on `WM_DESTROY` / `WM_ENDSESSION`, cleared by any notes
write). `TIMER_WIDGET_CARET` 46 on the note's own window (the caret blink,
only while it has the keyboard). `TIMER_STICKY_REMIND` 47 on the island window
(the reminders' one-shot, run A below). App
message `WM_APP_WIDGET = WM_APP + 71` (the event drain). Hotkey id 7 (6 is
the lyrics card's, merged on main; `WM_APP + 70` is the card's, 71 is ours). Idle cost: [[bata-idle-power-budget]].

## Ways to make a note

Settings -> Widget gallery (+), tray "Catatan tempel baru" (greyed at 20 with
the reason in the label), the seventh hotkey row (no default; a short island
notice at 20), the pin on a Notes card (`Hit::NoteSticky`, 22 dip, top-right:
accent when on the desktop), and the (+) in a note's header. Each makes an
empty Krem note (a right click on the header (+) or the gallery's (+) picks a
template, run A below), gives it the keyboard (except the pin, the note has
text) and turns `show` back on. Settings -> Widget: [[settings-structure]]. The hotkey:
[[hotkeys]].

The sticky note list (a header button, a tray item and an eighth hotkey row open
one window that lists and searches every note, shows and focuses one, deletes
with an undo): [[sticky-note-list]]. The header buttons are 20 dip with a 1 dip
gap since then (seven of them).

## Assumptions

- No drag-out-of-the-notch in phase 1.
- The cat peek paints the coat only, no accessory (a hat would be cut by the
  note's edge); on the note with an overdue reminder, else the note touched last
  (run A: Clock, Happy, Asleep, Looking, Idle moods), a still picture.
- Delete from the right-click menu is two clicks (the menu item reads "Klik
  lagi untuk hapus" on the next opening and the footer says it); it disarms on
  any other action.

## Demo

`--demo-widgets [reset]` (debug): its own notes file in
`%TEMP%\vale-widgets-demo\notes.json`, seeded notes (since run A eleven: all
papers, a 2 of 4 checklist, an all-done one, a rolled one, a pinned Gelap one and
the v2 things, see below), live
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

# Widgets, v2 ("Catatan tempel v2", run A, 2026-11-04)

Spec: `docs/design/catatan-tempel-v2.html` (mockups) and the owner's brief
(drag bug, features 1-7 and 11; run B is the clipboard and drop, stacks, tuck
and send to phone, not here). Every new thing is a field of the sticky part or
a pure module with tests; the window code only draws and routes.

## The drag bug, and the 36 dip strip

**Root cause.** `edit::create` makes the hidden EDIT as `WS_CHILD | WS_VISIBLE`
at the window's client (0, 0), sized to the text area in px. Every new note
gets the keyboard (`WidgetHost::focus` -> `begin_editing`), so that invisible
child covered the top-left of the window: the cat's margin and the strip from
x = 0 to about the text area's width (about 208 dip of a 240 dip note). Windows
sent the presses there, not to the note, and the six header buttons covered the
rest of the strip (from about x = 92), so a fresh note had no draggable spot at
all. A resize widened the note past the EDIT's fixed size, which is why
"harus di gedein dlu". The same child also took body clicks while editing.

**Fix.** The EDIT's subclass answers `WM_NCHITTEST` with `HTTRANSPARENT`, so
every press reaches the note's own procedure (IME placement is untouched).
And:

- The strip is `HEADER_H` 36 dip (was 28); `ROLLED_H` 64 (the strip and one
  28 dip line; it lives in `sticky.rs`).
- `paint::target(.., buttons_shown)`: with the buttons hidden (the header ease
  under 0.5) the whole strip, full width, is `Target::Header` (the at-rest pin
  of a pinned note is a status mark and drags too). The host passes
  `header.value(now) >= 0.5`. A hidden button never eats a press.
- A press on a button that then moves 4 px or more (`|dx| + |dy|`) becomes a
  Move drag from the press point (`Win::press_at`), `pressed` cleared so
  release is no click. Kunci posisi blocks it.
- Alt + press anywhere (text, box, footer, button, grip) starts a Move drag;
  `mouse_activate` answers `MA_NOACTIVATE` for it so no editing starts. Snap
  reads Alt on every move, so it comes back when Alt goes up mid-drag. The
  note swallows `WM_SYSKEYDOWN/UP` for `VK_MENU` and `WM_SYSCOMMAND`
  `SC_KEYMENU` (no menu mode, no beep). A note that does not have the
  keyboard never sees the Alt key at all: the foreground app does.
- Cursor: `IDC_SIZEALL` over the strip (not when locked) and during a Move,
  `IDC_SIZENWSE` for the grip and a resize, `IDC_HAND` over shown buttons and
  boxes.
- Unchanged: snap (8 dip grid, 12 dip gap, work-area clamp), Kunci posisi,
  double click on the strip rolls up.
- Every creation path was checked (Settings (+), tray, hotkey, the Notes pin,
  the header (+)): `w.note` and `w.view` are set by `relayout` inside `sync`
  before the first draw, and the only leftover state was the EDIT.
- Tests: pure `paint::target` (hidden buttons, 160 / 240 / 640 wide, rolled),
  a real EDIT answering `HTTRANSPARENT` (`edit.rs`), and "a new note can be
  dragged straight away" in `host.rs` (a real hidden window, `focus`, a strip
  point that `ChildWindowFromPointEx` says is the EDIT's, press, move, release,
  a `Placed` event). Tests that give a window the keyboard take a lock
  (`keyboard_lock`): activating one window deactivates another test's, which
  ends its editing.

## New fields in the sticky part (all skipped at their default)

| Key | Type | Default | Notes |
|---|---|---|---|
| `remind_at` | unix seconds | none | "Ingatkan aku" |
| `remind_fired` | bool | false | only meaningful with `remind_at` |
| `font` | `Vale` / `Tangan` / `Mono` | Vale | |
| `pattern` | `Polos` / `Garis` / `Kotak` / `Titik` | Polos | |
| `deco` | `None` / `Selotip` / `Paku` | None | |
| `stamp` | `None` / `Penting` / `Beres` / `Mendesak` / `Suka` / `Nanti` / `Ide` | None | |
| `opacity` | 100 / 80 / 60 / 40 | 100 | any other number snaps to the nearest allowed (a tie goes to the more opaque) |

`sticky::lenient` reads each on its own (a wrong type is the default; names are
trimmed, any case). A phase-1 file loads to the defaults and saves back with
exactly its old keys (tested). `WidgetPrefs.tilt` (config `widgets.tilt`,
lenient, default off) is written with the other prefs.

## Reminders (Ingatkan aku)

- `widgets/remind.rs` (pure, tested across midnight): presets 15 mnt, 1 jam,
  17.00 (tomorrow once 17.00 has passed, 17.00 exactly counts as passed),
  besok 09.00; `snooze` = +10 min; `nearest(notes)` = the earliest unfired
  `remind_at` (no `now`: a due one is `due`'s job); `due(notes, now)` also
  covers the ones missed while off or asleep; `overdue(sticky, now)` = fired and
  past; `wait_ms` clamped to `USER_TIMER_MINIMUM` .. `USER_TIMER_MAXIMUM`
  (`0x7FFFFFFF`, a clamped wait just recomputes on fire); `chip_label` ("17.00",
  "17:00", "Kam 09.00", "Thu 09:00"); `Entry` is the day / hour / minute
  stepper of "Atur sendiri" (5 min steps, wraps, never before today).
- **TIMER 47** `TIMER_STICKY_REMIND`, one one-shot on the island's window
  (`app_widgets.rs`). Armed for `nearest`, killed and re-armed on every
  reminder change (`widgets_sync` -> `sticky_remind_settle`, which also runs at
  startup), on `on_clock_jump` (`WM_TIMECHANGE`, resume) and on each fire (killed
  first). `sticky_remind_fire` also runs at startup, so the missed ones fire.
  It applies the windows' queued events first (`widgets_events`), so a reminder
  just cleared or moved in the menu does not ring at its old time. `reminded`
  (the notes with a notice) lives in memory only, so each fire also posts the
  notice, quietly (no raise, no shake), for every `remind::overdue_ids` note not
  in it: a note that rang before Vale was closed gets Tunda/Buka back.
  No polling. `app::tests::timer_ids_never_collide` asserts 47.
- A fire: each due note gets `remind_fired = true` (saved), the host raises it
  (`WidgetHost::raise`: `raised` makes `above_windows` true, so `apply_z` makes
  it topmost even on the desktop layer; a press on the note, Tunda, Buka,
  clearing or opening drops it; `sync` also drops it once `remind_fired` is
  false), it shakes 600 ms (`paint::wiggle_dx`, a decaying sideways swing of at
  most 4 dip inside the 8 dip margin, since a rotation would need margin; on the
  FrameClock only while it lasts, skipped when `!animate`), and the island gets
  one `NoticeKind::StickyReminder { id }`.
- The notice: sticky like `Reminder` (one slot per note id, cleared by
  `Event::StickyReminderCleared`, brief copy in fullscreen), no x, two chips
  `Tunda 10 mnt` and `Buka` (`SNOOZE_CHIP_W` 88 and the update offer's primary
  chip, drawn by `paint_update_offer`, hits `Hit::StickySnooze` /
  `Hit::StickyOpen`), a click on the body is Buka (`Effect::StickyOpen`), over
  an open panel it is the strip (the strip's click is Buka too), the robot face
  is Reminder's. It stays while the note is overdue and goes when the note is
  opened, snoozed, cleared, unstuck or deleted: `App::sticky_remind_settle`
  clears the notice of every note in `reminded` that is no longer overdue. No
  sound, no toast.
- Overdue = `remind_fired && remind_at <= now`: the header chip is in the accent
  (`sticky::chip_tones`, ink 4.5:1 on every paper and surface, tested) and the
  cat sits on that note with a small alarm clock. Opening the note
  (`begin_editing`, a click or Buka) clears the reminder (`Remind { at: None }`).
- Menu (`context_menu`, the ... button opens the same one): "Ingatkan aku" with
  the four presets, "Atur sendiri..." (`remind_pop.rs`, a layered popover in
  the colour popover's style: day, hour, minute steppers, the wheel works,
  Simpan disabled unless the time is ahead) and "Hapus pengingat" (grey with no
  reminder). Each change is `WidgetEvent::Remind`.
- `--demo-widgets` has no island: the demo owner handles 47 itself (raise and
  shake, no notice).

## Fonts, paper, decoration, tilt

- `NoteFont`: Tangan is Ink Free if `fonts::system_has`, else Segoe Print, else
  Vale's face (`sticky::tangan_family`, tested); Mono is the bundled DM Mono
  (`Role::Mono`). No new font file, the exe does not grow. `Res` keeps Vale's
  `Face` and makes the other two lazily; the format cache is keyed by
  (font, size, weight); `body_key` includes the font; the title and the rolled
  title use it.
- `Pattern` (Garis, Kotak, Titik): `art::draw_pattern`, vector rules in the
  text area, scrolling with the text, under it. Garis falls under every text line
  (pitch `line + 1`), Kotak and Titik use a 12 dip grid. Line colour
  `Tones::line` = a hair of ink on the fill (8 %) and 10 % of the island text on
  Gelap; every text tone keeps 4.5:1 over it on every paper and surface (tested).
- `Deco`: `Selotip` (a washi strip of accent and light stripes, leaning -3
  degrees) and `Paku` (a round pin) drawn over the top edge in the margin. Their
  centre is the top centre, moved left on a narrow note so they never meet the
  overdue alarm clock left of the cat (`art::clock_centre` +- `CLOCK_REACH`, 2
  dip gap) nor the cat's face (`art::deco_centre`, tested at every width; the
  room is kept whether or not the cat or clock is there, so they do not jump).
  The deco is painted after both, so overlapping would hide the clock. The tape
  is centred from 246 dip, the pin from 201.
- `Stamp`: round, -12 degrees, bottom right of the text area above the footer,
  clear of the grip (`art::stamp_centre`, tested); a line icon (star, check, !,
  heart, cup, sparkle: `Icon::Tick`, `Mark`, `Heart`, `Cup` were added to
  `ui/icons.rs`, the others exist) and the uppercase label (smaller for a long
  word). Ink `art::stamp_ink`: the accent toward the ink on pastels, the island's
  accent on Gelap; 3:1 on every paper, surface and accent (tested). Chosen from
  the right-click menu "Cap" as radio items.
- Tilt ("Miring sedikit", Settings -> Widget, off by default; the Widget card has
  six rows now): `art::tilt_deg(id)` (FNV-1a with a splitmix finaliser, within
  +-2 degrees, notes differ). `Win::margins()` is one function per note used by
  `canvas_px`, the origin, `client_to_dip`, `screen_point`, the IME point and the
  drag's `present`: the usual margin plus the rotated overhang
  (`art::margins_for`). `paint::draw` rotates about the note's centre
  (`art::tilt_transform`); `target_at` and `client_to_dip` map the pointer through
  `art::unrotate` before `paint::target` (tested: a corner maps back to its
  Target, a round trip is the identity). A note with the keyboard is drawn and hit
  straight (`Win::lean()` is 0) in the same window (the margin does not depend on
  editing, so no resize). Known approximation: `PushAxisAlignedClip` under a
  rotation uses the rotated rect's bounding box, so a leaning note that is
  scrolled can show text up to about 4 dip into the strip or footer.
- The colour popover (`pop.rs`) grew to 292 x about 290 dip (`pop::open` clamps
  it inside the work area): swatches, then segmented rows Ukuran teks, Huruf
  (Vale / Tangan / Mono), Kertas (Polos / Garis / Kotak / Titik), Hiasan and
  Tembus pandang (100 / 80 / 60 / 40 %), then the two switches. Segment widths
  hold the longest label in both languages (tested with real text).

## Progress, confetti, cat moods, templates, see-through

- A note with items shows a 2 dip bar right above the text (`art::progress_rect`,
  full width minus `PAD_X`, accent on a faint track) and "done/total" after the
  date at rest (accent when all done).
- Confetti: `art::confetti(seed, t_ms, w, h)` (pure, seeded: 40 pieces fly up
  from the middle and fall, each clamped inside the window, empty from 800 ms)
  starts only from `toggle_line` when a tick takes the note from not-all-done to
  all-done, and only when `animate`. It runs the FrameClock while it lasts
  (`moving()` counts it) and is drawn in window coordinates, not leaning.
- Cat moods (`widgets/mood.rs`, pure): `mood(overdue, all_done, touched, now,
  pointer_over)`: Clock, Happy, Asleep (touched 3 days ago or more; never touched
  is not asleep), Looking, Idle, in that order. `cat_owner`: the note with an
  overdue reminder (the earliest), else the last touched. Looking turns the eyes
  and head toward the pointer in `look_step` quantised steps (-2..2), recomputed
  only from `mouse_move` on that note and redrawn only when the mood or the step
  changes. Asleep is re-evaluated by `WidgetHost::mood_tick(unix)` on the
  existing 1 s `TIMER_SYSTEM` tick, next to the fullscreen check: it redraws only
  the cat's note and only if its mood changed. The demo calls it from its sync.
- Templates (`widgets/templates.rs`, pure): `Template { Kosong, Belanja, HariIni,
  Rapat, Ide, Mingguan }`, `text_of(lang, t, today)`, `caret_of` (end of the first
  line). A right click on the header (+) opens a native menu (`WM_RBUTTONUP` on
  `Target::Button(Plus)`), its choice is `WidgetEvent::NewBeside { id, template }`;
  a left click stays Kosong. Settings' gallery (+) opens the same menu under the
  button (`After::TemplateMenu`, `host::template_menu`) and posts
  `Action::WidgetAdd(Template)`. `App::new_sticky(beside, template)` fills the
  text through the store and focuses the caret at `caret_of`. The tray, the hotkey
  and the Notes pin stay Kosong. They are plain text, so the Notes page reads them
  (tested through `first_line`).
- See-through: `Win::alpha` is an `Ease` of the layered window's alpha: the
  note's `opacity` % while the pointer is away, full over it, with the keyboard,
  raised or dragged (`alpha_goal`, `settle_alpha`). Only `present` moves, never a
  redraw (`present_alpha`); eased on the FrameClock about 120 ms, instant when
  `!animate`. Counted in `moving()`.

## Power (run A)

No repeating timer and no idle redraw. The one new timer is the one-shot 47.
The wiggle, confetti and see-through fade run the FrameClock only while they
move (`host.moving()`); the mood check rides `TIMER_SYSTEM`; the pointer look
rides mouse moves. See [[bata-idle-power-budget]].

## Demo (run A)

`--demo-widgets [reset]` seeds eleven notes: the six of phase 1 (now with stamps
and a ruling) plus a reminder in 3 hours, an overdue one (the clock cat), Tangan
with Kotak and tape, Mono with Titik and a pin, a 60 % see-through one with a
heart, and a Kotak checklist (0 of 3). `DI_WIDGETS_TILT=1` turns the lean on. The
island notice has its own scene, `sticky-reminder`, in the demo sheet.

## Verified and not verified (run A)

Verified: `cargo test --locked` (pure modules, the real-window host tests above,
settings layout), and the debug demo on this PC captured window by window with
`PrintWindow` (every paper, the chip, the clock cat, the progress bars and
counts, the stamps, the rulings, the tape and the pin, Tangan and Mono, the lean
with `DI_WIDGETS_TILT=1`, Indonesian and English).
Not verified live: a real mouse drag and Alt-drag, the Alt key-up swallow, a
reminder over other apps and after sleep or a clock change, the wiggle and
confetti as motion, the see-through over real windows (alpha does not show in
`PrintWindow`), Ink Free's look on a machine that lacks it (it was on this one),
the colour popover and the "Atur sendiri" entry on screen (their layout and
labels are tested; they were not opened because they take the foreground), the
native menus, the island notice on the real island.

## Run B: stacks ("Tumpuk catatan", 2026-11-04)

Spec: the owner's brief (run B, feature 9) and `docs/design/catatan-tempel-v2.html`
page 3, card 9. Built together with the tuck below, because both rework the one
window per note model, the drag release and the painting.

### Data (sticky part, all skipped at the default)

| Key | Type | Default | Notes |
|---|---|---|---|
| `stack` | string | none | the stack id; a made stack is `stk-` + the target's note id |
| `stack_order` | u32 | 0 | the sheet's place, 0-based |
| `stack_top` | bool | false | the sheet the stack shows |
| `tuck` | `left` / `right` | none | any case on read, a wrong value is none (below) |

`sticky::lenient` reads each on its own. A phase-1 and a run A `notes.json` load
to the defaults and save back with exactly their old keys (tested with literal
JSON). Every sheet is an ordinary note on the Notes page.

### Units, windows, retarget

- A *unit* is what has a window: a plain note or a whole stack (`stack::units`,
  sheets by `stack_order` then id). A stack id that only one live sticky sheet
  carries is a plain note.
- `Win::id` still means "the note id of the sheet this window shows" (a dropped
  clip or a send-to-phone acts on it). `Win::stack: Option<StackView { id, ids,
  pos }>` names the stack; `Win::sheets` keeps every sheet's sticky part, text
  and edit time (the shown sheet's copy is stale, the window's own fields are
  live). `Win::key()` is the stack id, else the note id; the lean uses it, so a
  flip does not change it (`art::tilt_deg(stack id)`).
- **A flip, or a new shown sheet, retargets the window**: editing ends first
  (`end_editing`, and a popover of the note closes), then `id`, `sticky`, `text`
  and `updated` are swapped, `body_key = None`, the scroll, selection and footer
  message reset, `dirty`. The window is never destroyed or recreated. The leaving
  sheet's live text is written back into `sheets` first, and place, size, pin,
  roll and tuck of the new sheet are the stack's (`stack::share`).
- `sync` matches windows to units by key first, then (`stack::match_units`) by a
  sheet the window holds, preferring the window that shows the unit's top sheet:
  a note that joined a stack, or a stack that came apart, keeps a window and the
  others are destroyed or made. The `.take(MAX_STICKY)` still counts sheets.
  `count()`, the cap (`sheet_total`), the tray and Settings are sheet counts.
- The cat, a raise, `focus` and `mood_tick` map a hidden sheet's id to its unit's
  window (`Win::has`); the cat's mood and `raise` look at every sheet's reminder.
  Buka on a hidden sheet brings it to the top first. See-through uses the shown
  sheet's `opacity` (`settle_alpha` runs on every flip).

### Pure rules (`widgets/stack.rs`)

`normalise` (one sheet shown: the first flagged, else the first; orders 0..n-1; a
lone stack id cleared), `shown` (an overdue sheet comes to the top, the earliest
if several), `join` (the stack id is the target's, else `stk-` + its note id; the
dragged sheets first in their own order, then the target's; the shown sheet is the
dragged unit's; every sheet takes the target's rect, monitor, pin, roll and tuck;
width at least `MIN_W` 260), `flip` / `flip_pos` (wraps), `show`, `take_out` (the
sheet is placed with the (+) rule, right, else left, else below, keeping its own
size, untucked; the next sheet in order shows if it was the shown one), `leave`
(what the rest becomes when a sheet is deleted or unstuck), `propagate` (a Placed,
Pinned, Rolled or tuck change on one sheet is written to every sheet),
`wheel_flips`, `match_units`. `apply.rs` applies the new events (`Stack` and
`StackOut` structural; `StackShow` and `Tuck` not), runs `settle` (= `normalise`)
at the end of every apply and in `widgets_sync`, and `Outcome::unstack` carries
the sheets to take out: the owner does it with the displays (`apply::stack_out`).

**The overdue sheet comes to the top at startup and when a reminder fires, not on
every sync.** `settle(.., overdue_top)` is true only from `widgets_start`,
`sticky_remind_fire` and the demo's start. Always applying it would undo a flip
away from an overdue sheet at the next unrelated sync.

### Making a stack: Shift, not a rest

Run B allowed either "rest 0.5 s over the target" or Shift. **Shift is the
trigger.** A rest needs a clock while the pointer stands still (a timer or the
frame clock for half a second of nothing), and it stacks whenever one drags over
a note and hesitates. Shift is read on every move, like `alt_down` (`shift_down`,
with a test hook), costs nothing, and cannot stack by accident.

- During a Move drag with Shift held and the pointer inside another unit's shown
  rect (`Win::note`, not the margin; tucked units are left out), that unit is lit
  with the highlight frame, static while targeted (`Win::targeted`, `Scene::
  highlight` = 1).
- Release with Shift still down over it: `WidgetEvent::Stack { dragged, target }`
  (the two units' ids, in order), no `Placed`: the join places it.
- Without Shift a drag never stacks, with or without snap. Snap, Alt and Kunci
  posisi are as in run A. A drag that ends because the window lost the mouse
  never stacks.

### Drawing

- Up to two paper edges peek out under the note at (+3, +3) and (+6, +6) dip, in
  the fill of the next sheets in order, with a soft shadow, inside the 8 dip
  margin, in the note's own transform (they lean with it).
- The strip's left, where the date sits, shows "1 / 3" (tabular figures:
  `Res::layout_tabular`, `fonts::tabular_figures`) between two 20 dip arrows
  `Btn::Prev` / `Btn::Next` (`paint::stack_strip`: arrow 6..26, counter 28..74,
  arrow 76..96). The counter is always drawn; the arrows fade with the other
  buttons (the header ease) and, like them, take no press while hidden, so the
  strip drags (`paint::target_in(.., stack)`). The date and the checklist count
  are not drawn on a stack. At `MIN_W` 260 the Pin button starts at 112, so the 96
  end of the arrows clears it by 16 dip (tested with the real font for "20 / 20").
- The wheel over a stack's strip (`WM_MOUSEWHEEL`, whose position is in screen
  coordinates) flips one sheet per 120 delta, accumulated (`Win::wheel`): down is
  next, up is previous, wrapping. Over the text it scrolls the text. A flip is
  saved through `StackShow { id }` as `stack_top`.
- Context menu: "Keluarkan dari tumpukan" (id 24) only in a stack, right after
  Roll.

### Verified and not verified (stacks)

Verified: `cargo test --locked`: the pure module (ordering, normalise, overdue,
join, flip with wrap, show, take-out, leave, propagate, wheel, matching, the cap of
a 3-sheet stack plus 17 notes), `apply` (events, structural, propagate, delete and
unstick), the paint geometry (counter and arrows at `MIN_W`, hit-testing of the
arrows), and real hidden windows in `host.rs`: a plain drag over a note does not
stack, a Shift release does (the dragged window survives, one window, two sheets),
a stack drag moves all sheets, the wheel on the strip flips and retargets without
a new HWND, the arrows, the lean by stack id, the cap, a stack coming apart, the
hidden sheet's id reaching the unit's window.
Also run on this PC, in the debug demo with `PrintWindow` and window messages sent
to the windows (the real pointer was not moved): the stack drawn (counter "1 / 3",
the Laut and Plum edges under the Krem sheet), and `WM_MOUSEWHEEL` over its strip
flipping the one window 1 / 3 -> 2 / 3 -> 3 / 3 and back with the new text, paper
and peeking edges each time.
Not verified live: a real mouse drag with a real Shift key, the wheel from a real
wheel.

## Run B: tuck to the edge ("Selip ke tepi", 2026-11-04; pop 2026-11-05)

**2026-11-05: the horizontal slide is gone.** A tucked note now grows out of its
tab as a scale and fade (the owner picked option C of the mockups): see "Pop"
below. Everything else here (outer-edge rule, grace, `keep_out`, drag, menu,
demo) is as before.

Spec: feature 10, page 3 card 10 of the mockup.

### Geometry (`widgets/tuck.rs`, pure, dip in the placement area)

- `Edge { Left, Right }`, saved as `left` / `right`.
- **Outer edge rule.** A note tucks only to an edge that no other display's
  *work area* adjoins across the note's vertical span (`tuck::adjoin`, physical
  px, 1 px slack for rounding). On an inner edge the note simply crosses to the
  next monitor. A smaller display beside only the upper part leaves the lower
  part of that edge outer.
- `press_edge(note, area, cursor, adjoin)`: a Move drag tucks when the note's
  left (or right) side sits at the area's edge within 0.5 dip after the snap
  clamp and the cursor is within 4 dip of, or past, that edge, and the edge is
  outer. A note as wide as the area goes to the side the pointer is on.
- `menu_edge`: "Selipkan ke tepi" uses the nearer outer edge to the note's
  centre, the other side if the nearer is inner, greyed when neither is outer
  (and with Kunci posisi).
- `out_rect(view, edge, area)`: x flush with the edge, y clamped into the area.
  `tab_rect(out, edge)`: 16 dip wide, `min(72, shown height)` tall, at the note's
  top, flush with the edge.
- **Pop (`tuck.rs`, pure).** Progress p runs 0 (only the tab) to 1 (out), linear
  in time. The note is scaled about `origin(edge, w, shown_h)` (the tab's vertical
  centre on the edge side: x = w on the right, 0 on the left, y = min(72, h) / 2)
  and faded; the tab stays unscaled and fades the other way.
  - Way out: 300 ms (`OUT_MS`), `out_scale(u) = 0.12 + 0.88 * back_out(u)`,
    ease-out-back with `BACK` 1.1, so it overshoots to about 1.039 (peak at
    u = 0.65; `PEAK_SCALE` = 1.0394) and settles at 1. Way in: 200 ms (`IN_MS`),
    `in_scale(u) = 1 - 0.88 * u^3` (ease-in cubic, no bounce). `MIN_SCALE` = 0.12.
  - Opacity is a function of p, the same in both directions (the way in is the
    mirror of the way out): `note_alpha(p) = smoothstep(p / 0.45)` (the note is
    solid from 45 % of the way), `tab_alpha(p) = 1 - smoothstep(p / 0.35)` (the
    tab is gone at 35 %).
  - `Pop { from, to, from_scale, start, ms }` replaces the old `Ease` for the
    slide. `toward(to, now, animate)` starts from the current progress **and the
    current scale**, so a reversal part way has no jump in scale or opacity, and a
    partial trip takes proportionally less time (`ms = 300 or 200 * |to - from|`);
    without animation it is instant (ms 0). `scale`, `note_alpha`, `tab_alpha`,
    `progress`, `running` are all of `now`.
  - `scale_about` / `unscale` (the exact inverse) map points through the scale.
  - `room(w, shown_h, edge)`: the extra window margin (dip, rounded up) the
    tucked note's paper plus shadow needs at `PEAK_SCALE` about the origin
    (`SHADOW_REACH` 6 / 4 / 6 / 8: the shadow reaches 5 dip around and sits 2
    down, plus 1 slack), over the base margins (8 each side, 30 more on top).
    A 240 x 208 note needs about 8 on the open side and 8 at the bottom, never
    anything on top.
- **Place across a monitor change** reuses `place::place`: a tucked note's view
  is replaced by `out_rect` on the display it lands on (the same edge, the y
  clamped there, a rolled note at its shown height). The fallback to the primary
  monitor still does not rewrite the save: a note tucked right on a missing
  3840 wide monitor sits flush right on the primary and is flush right on its own
  monitor again when it returns (tested with two fake displays and a missing
  one). `place::others_on` leaves tucked notes out (snap neighbours, where a new
  note goes).

### Behaviour

- The window stays at the out rect. Tucked all the way in (`Win::pop` at
  progress 0) only the tab is drawn (`paint::draw_tab`): paper fill rounded on its
  inner side, a 1 dip ink line on the inner side, three short grip lines, an
  accent dot while a reminder of the unit is overdue; every other pixel has alpha
  0, so it is click-through. The cat, the tilt and the paper edges are not drawn
  on the tab. Popping draws the note scaled about the tab (`TuckScene { edge,
  progress, scale, note_alpha, tab_alpha, overdue }`; the transform is the scale
  about the window point of `origin`, then dip to px) inside a Direct2D layer at
  `note_alpha` while that is under 1, then the tab unscaled on top at `tab_alpha`.
  `Scene.dx` is only the reminder's wiggle again. A tucked note never leans (and
  an untuck that animates does not lean mid-scale: `Win::lean()` is 0 while the
  pop runs), and the cat shows only when it is all the way out and settled.
- **Hit keeper.** The layered window only gets the pointer where alpha is above 0,
  and part way out there is a gap where the faded tab was and the scaled note does
  not reach yet (at scale 0.9 its top starts about 3.6 dip below the tab's top).
  While tucked and 0 < progress < 1, `paint` fills the tab rect at 3/255
  (`tuck::KEEPER_ALPHA`, about 0.012, over the faded tab). A pointer resting there
  keeps the window, so no `WM_MOUSELEAVE` fires and the note stays out.
- **Window room.** `Win::margins()` is the base `art::margins_for` plus `Win::room`,
  which `draw` sets to `tuck::room(..)` while the note is tucked or its pop runs
  (`tuck_edge(now)`: `sticky.tuck`, else `pop_edge` while the pop runs, so an
  untuck can still be drawn). The canvas is resized by the existing check in
  `draw` and `origin()` follows, so a tucked note is never clipped at the
  overshoot; the extra margin is alpha 0, so it is click-through. `room` is
  stored (not derived from the clock) so that `origin()` always matches the
  window as it was last drawn; the room is given back by the frame after the pop
  ends.
- Hover: `WM_MOUSEMOVE` over the tab pops it out (300 ms ease-out-back; the
  existing `TrackMouseEvent(TME_LEAVE)` arms the leave). `WM_MOUSELEAVE` while out
  starts a 300 ms **grace** (`Win::grace`), then it shrinks back into the tab
  (200 ms ease-in). Re-entering cancels the grace, or reverses a pop part way
  with no jump.
- **No new timer.** The grace and the pop run on the owner's FrameClock: they
  are counted in `WidgetHost::moving()` (`grace.is_some() || pop.running(now)`),
  the owner starts the clock on `NeedsClock`, `tick` ends the grace and starts the
  pop back in, and nothing is drawn during the grace (a frame is drawn only when
  the drawn values, `pop_drawn` = scale, note and tab opacity, or the window room,
  differ from what was last drawn, so the last frame of the pop is drawn too).
  Without animation (Motion not Full, Energy Saver, Windows animations off) the
  pop is instant and the grace still applies.
  `app::tests::timer_ids_never_collide` is unchanged.
- **Hit testing while it pops.** `target_at` first checks the tab rect: while a tab
  shows (progress under 0.999) a point in it is `Target::Header`, so the tab counts
  as part of the note all the way up (the keeper holds the pointer there). Otherwise,
  while tucked or popping, the note point goes through `tuck::unscale` by the
  current scale before `paint::target_in`; at progress 0 anything outside the tab
  is `Outside`. `client_to_dip` uses the same inverse (no shift). `Win::on_tab`
  (the tab shows, `tab_alpha` above 0, the point in `tab_rect`) is what a press, the
  cursor and `mouse_activate` use: a press on the visible tab shows the note out at
  once (`pop_out_now`) and starts a Move. A press anywhere else on a note that is
  part way maps through the inverse scale, then the pop is finished at once and the
  press is handled as on a note that is out (a drag from any press does the same,
  in `begin_drag`). The wheel still ignores flips while the tab shows.
- It stays out while it has the keyboard, is dragged, is raised by a reminder, a
  popover of it is open, or the pointer is over it (`keep_out`); `tuck_settle`
  runs wherever one of these may have ended (leave, end of editing, `lower`, a
  popover closing, the end of a drag, a sync) and starts the grace.
  `WidgetHost::raise` (a due reminder, also by a hidden sheet's id) and `focus`
  pop it out (300 ms) and keep it out.
- A press on the tab starts a Move drag with the note shown out at once (Kunci
  posisi blocks it; `mouse_activate` gives no keyboard). A Move drag that ends
  without pressing an outer edge untucks (`Tuck { edge: None }`) and places the
  note where dropped; one that ends pressing an outer edge tucks (`Placed` flush
  with the edge, then `Tuck`). While a release would tuck, the dragged note shows
  a 3 dip accent bar on that edge (`Win::cue`). A drag that did not move changes
  nothing. Tucked units are excluded from the snap neighbours and from the stack
  targets. A stack tucks as one (the tuck is written to every sheet).
- Context menu, right after Roll (and after "Keluarkan dari tumpukan"): "Selipkan
  ke tepi" / "Tuck to edge" (id 25; greyed with Kunci posisi and when neither side
  is outer), "Keluarkan" / "Bring out" once tucked (same id), which untucks in
  place: the note pops out of its tab (`pop.toward(1.0)`, 300 ms, instant without
  animation, nothing if it was out) with `pop_edge` kept for drawing until the
  pop settles.
- Tucking by a drag release on an outer edge or by the menu leaves the note out
  (pop at 1, `pop_edge` set); once nothing keeps it out the grace runs, then the
  200 ms scale-in into the tab. (Assumption: the grace still applies before the
  tuck animation, as before.) The apply path (a tuck change from the owner's data)
  sets `Pop::at(1)` and `pop_edge`, as it set the slide.
- Tucks survive a restart (the sticky part) and a monitor change (above).

### Demo

`--demo-widgets reset` seeds fifteen notes without the clipboard sample note (sixteen with it, see "Integration (Run B finish)"): the eleven of run A, a stack of
three sheets ("Proyek Dago", Krem, Laut, Plum, sheet 1 shown), and one note tucked
on the right edge of the notch's monitor. The demo owner gets `NeedsClock` like the
island, so the pop and the grace run on its own frame clock: hover the tab of the
tucked note and it grows out of it. Hold Shift while
dragging a note onto another to stack; the wheel over a stack's strip flips.

### Verified and not verified (tuck)

Verified: `cargo test --locked`: `tuck.rs` (press_edge outer against inner, the
0.5 and 4 dip limits, adjoin across a span, menu_edge, out_rect clamp, tab_rect;
the pop: `out_scale` 0.12 to 1 with a peak above 1 and at most 1.041 that
`PEAK_SCALE` covers, non-increasing after it, `in_scale` monotonic, `note_alpha`
and `tab_alpha` at 0, 45 %, 35 % and 1, `origin` per edge and for a rolled note,
`unscale(scale_about(p))` round trip, a full `Pop` trip equal to the curves, a
reversal part way in both directions with no jump in scale or opacity, a half
trip taking half the time, no animation instant, `room` for 240 x 208 on both
edges and 0 on top), `place.rs` (flush on another display, a shorter display, a
missing monitor and its return), and real hidden windows: hovering the tab grows
the note (scale rises from 0.12, `moving()` true while it runs and false once
settled, with and without animation), the grace then the 200 ms scale-in, nothing
drawn during the grace, re-entering part way reverses without a jump, a click part
way maps through the inverse scale to the body, header and grip (and the tab rect
is `Header` while the tab shows), a press part way finishes the pop, tucking by
the menu ends with the scale-in after the grace, bringing a tucked note out pops
(instant without animation, no lean mid-scale, the room given back after), the
margins include `room` and the peak-scale paper plus shadow fit in the canvas,
staying out for the keyboard and a raise, a drag that ends on an outer edge tucks
and elsewhere untucks, the cue, an inner edge never tucks, the menu's edge, a tab
is not a snap neighbour, the frame clock stays off at rest.
Also run on this PC in the debug demo (`--demo-widgets reset`, `PrintWindow` and a
sent `WM_MOUSEMOVE` to the tucked note's window, the real pointer not used): the
tab alone (Sage paper, grip lines); then frames at 60, 120 and 180 ms with the note
scaled about the tab on the right edge, translucent while it grows, full size and
unclipped at 180 ms (window 264 x 262 dip: the note's 256 x 254 plus the 8 dip of
room on the open side and at the bottom). A sent move is not a real pointer, so
Windows' own `WM_MOUSELEAVE` came at once and the grace and the scale back in ran:
the note was a tab again at the last frame.
Not verified live: a real mouse drag to a real screen edge, hover with a real
pointer that stays (the hit keeper at 3/255 is tested as geometry only: no pixel
test of the alpha, and that Windows really keeps the pointer in the window there is
not checked), the look of the pop as motion (only still frames), two real monitors,
the tab at 150 % DPI.

## Run B: clipboard prefill and drops

Feature 8 of sticky notes v2 (2026-11-04). New pure module `cliptext.rs`, new
Win32 module `drop.rs`; [[clipboard-privacy]] is the rulebook for the first.

**The setting.** Settings -> Widget -> "Di desktop", a seventh row right after
"Miring sedikit": "Isi dari clipboard" / "Fill from clipboard", off by default
(`WidgetPrefs::clip_fill`, config key `widgets.clip_fill`, read leniently per
field like `tilt`: a wrong type is false). Wired as `Hit::WidgetsClipFill` ->
`Action` -> `SettingsAction::WidgetsClipFill` -> `App::widgets_clip_fill` ->
`save_config`. The card is 7 rows now (the section tests count them). See
[[settings-structure]].

**Who prefills.** Only `on_sticky_hotkey` and `on_sticky_menu` (the tray). The
Settings gallery (+), a header (+), the Notes page pin and the templates never
do. With the setting on, `App::sticky_prefill` reads the clipboard once, at that
moment, with `clipboard::win::read_now(island hwnd)`: `read` minus the
"own copy" check (a card copied from the history is a fine text) and minus the
picture bytes, and a text over the history cap is read as a bounded prefix (16 x `CLIP_MAX` UTF-16 units; `read_text(true)`) rather than skipped. No listener, no retry timer, no polling; an `OpenClipboard` that
fails (`Read::Busy`) gives an empty note. It is not read at all at the 20-note
cap. The text is never logged.

**What counts.** `cliptext::from_snapshot`: the `kinds::should_skip` rules
(`ExcludeClipboardContentFromMonitorProcessing` or `Clipboard Viewer Ignore`
present, `CanIncludeInClipboardHistory` or `CanUploadToCloudClipboard` 0 or
unreadable) give nothing; so do files and pictures. Only Unicode text starts a
note. `new_sticky_with(beside, Template::Kosong, Some(text))` sets the text like
a template and puts the caret at the end (`focus_at(id, None)`); `new_sticky`
stays the same for its other callers.

**`cliptext::prefill`.** CRLF and CR to LF; control characters other than LF
dropped, TAB to 4 spaces; trailing whitespace trimmed on every line; blank
lines at both ends and the outer whitespace trimmed; more than 2 blank lines in
a row become 2; capped at `CLIP_MAX` = 2000 chars (Unicode scalars, so emoji
never split): cut at the last LF in the final 200 chars before the cap if there
is one, else at the cap; no ellipsis; empty is `None`. The first line is the
title, as the checklist rules already say. `normalize` is the same without the
cap (the drop reuses it).

**Dropping onto a note.** Every note window is an OLE `IDropTarget`
(`drop.rs`, `#[implement]`, like `shelf_ole`). `drop::register(hwnd)` runs right
after `create_window` succeeds; `drop::revoke(hwnd)` is the first thing in the
note window's `WM_DESTROY`, so every destroy path (a note leaving the desktop,
`shutdown`) revokes. Formats, in priority: `CF_HDROP` (each full path on a
line, at most 32), `UniformResourceLocatorW`, then `UniformResourceLocator`
(ANSI, decoded with the system code page via `MultiByteToWideChar(CP_ACP)`), then `CF_UNICODETEXT`. Anything
else: `DROPEFFECT_NONE`, no outline. The answer is LINK for files when offered
else COPY (`shelf_ole::choose_effect`); COPY for a link or text, NONE when COPY
is not offered. **Never MOVE**: a move would make the source app delete what
was dragged. `shelf_ole::read_paths_max` is the one shared helper (the shelf's
`read_paths` calls it with its own limit).

**OLE init.** `ole_ready()` calls `OleInitialize(None)` once per thread before
the first `RegisterDragDrop`. On the island thread COM is already an STA
(`app.rs` `CoInitializeEx`), so it answers S_FALSE, as `shelf_ole::register`
does; the `--demo-widgets` thread and test threads get OLE from it. A failure
(a thread in MTA mode) is logged once with `eprintln!` and the note simply takes
no drops; never fatal, never a panic.

**What a drop does.** Pure `drop_lines(DropKind)`: text through
`cliptext::normalize` with blank lines removed; a link one trimmed line (the
first non-empty one, control characters out); files trimmed, at most 32.
Pure `append_lines(text, &lines)`: empty note is the lines joined with LF,
otherwise text + LF (none if it already ends with one) + lines; the added block
is capped by `CLIP_MAX` (the note itself is not). `host::drop_into(hwnd, lines)`
applies it: without the keyboard `Win.text` is set and `text_changed` draws and
pushes `WidgetEvent::Text`; with the keyboard (`Win.edit` is Some) the edit goes
through `apply_edit` (selection moved to the end, `EM_REPLACESEL`, undo works).
Both then `touch`. A rolled note stays rolled; Kunci posisi does not block a
drop (it locks the place only). The window's `Win::id` note gets it, so a drop
onto a stack goes to the top sheet.

**Highlight.** `Win::drop_hover`, set by an accepted `DragEnter`, cleared on
`DragLeave` and `Drop`. `paint.rs` strokes a 2 dip accent outline, dash 6 gap 4
(a custom stroke style of 3 and 2 stroke widths, made once in `Res`), inset 1
dip along the note's rounded rect (`drop_frame`). The callbacks reach the host
only through `with` (`try_borrow_mut`; a busy state skips the redraw) and never
borrow `App`, because the shelf's own `DoDragDrop` may be pumping. No new
WM_APP message, no timer id, no hotkey id.

**Demo.** `--demo-widgets` seeds a twelfth note from a fixed sample string run
through `cliptext::prefill` (an address and a line, with CRLF and trailing blank
lines). The demo and the tests never read or write the real clipboard.

**Verified.** `cargo test --locked`: cliptext (CRLF, controls, tabs, blank-line
collapse, the cap at a line break and on an emoji boundary, empty to None, the
exclusion rules on `ClipSnapshot` fakes), `drop_lines`, `append_lines`, `pick`,
`effect_for`, the lenient `clip_fill` read, the Settings layout tests (fit in
both fonts and languages, viewport at 720x520, none in a tall window, last card
meets the bottom margin), and real-window host tests: a hidden note takes a drop
(Text and Touched events, rolled and locked), a note with the keyboard gets it
through its EDIT (caret at the end, EM_UNDO takes it back), the outline flag,
and that each note window registers one target and revokes it when destroyed (a
live-target counter, which also proves OLE starts on a test thread).
`timer_ids_never_collide` is unchanged.

**Not verified live:** a real drag from Explorer, a browser (link, text) or an
editor onto a note, and the look of the dashed outline on screen; reading the
real clipboard (no test or demo may touch it) for the hotkey and tray paths,
including a password manager's copy; the ANSI link format from a real source.


## Run B: send a note to the phone

"Kirim ke HP" (EN "Send to phone") shows one sticky note on the phone link as a
read-only page. Spec: the owner's brief (feature 12), mockup page 3 card 12 of
`docs/design/catatan-tempel-v2.html`. The server side is in [[shelf-phone]]
("Run B: a note on the phone").

- **Menu.** `CMD_SEND_PHONE = 26`, one item in the note's context menu between
  the Cap submenu and Pin; the ... button opens the same menu. It pushes
  `WidgetEvent::SendToPhone { id }`; `apply` puts it in `Outcome::send_phone`
  (the last one in a batch wins). `--demo-widgets` has no island and no phone
  link: it prints a line and ignores it.
- **Snapshot rule.** `App::phone_send_note(id)` (`app_phone.rs`) builds a
  `phone::NoteShare` from the note as it is when the item is chosen:
  `NoteShare::from_note(text, paper, island, accent, pc)`. Title = the first line
  (a checklist marker on it is dropped, `checklist::title_of`); the lines after it
  in order, `- [ ] ` / `- [x] ` / `- [X] ` as items (the `checklist.rs` marker
  rules, a marker with no space after it stays text), trailing blank lines
  dropped. Colours come from `sticky::tones`: fill and body ink, `soft` =
  `tones.secondary` for the footer and done items. **Gelap** is the fixed fill
  #1B1D1F with the island's light text tone (`island.text`) so the page looks the
  same whatever surface the island wears; the contrast of ink and soft on the
  fill is >= 4.5:1 for every paper, surface and accent (test in `note_page.rs`).
  The page shows the note **as it was when sent**; sending again refreshes it.
- **One note at a time.** A later Kirim ke HP replaces the snapshot
  (`Handle::share_note(Some(..))`).
- **Reuses the session.** If no session runs, `phone_begin` starts one exactly
  like the shelf HP pill; the snapshot is handed over at once (it lives in the
  session's own state, so it is there before `Ready`). Then
  `Event::PhoneAttention(PhoneStage::Qr)` opens the island panel on the Shelf
  page at the QR view and latches it like a drop (nothing in fullscreen). With
  a session already running the QR view shows too. No new timer, thread or poll.
- **Clearing.** `Handle::share_note(None)` and `PhoneState::note` (the note id)
  go when: the note is deleted or taken off the desktop (the apply `Outcome`'s
  `gone` list, `phone_note_gone`), or a sync finds it is no longer a desktop
  note (`phone_note_check` in `widgets_sync`, which covers the Notes page's pin
  and delete); a refused start; the session ending (`forget_session` and the
  server's `end`). Editing the note afterwards does not change the page.
- **Not touched:** the shelf's files, uploads, downloads, thumbnails, page.html,
  share ids, the token and the idle rules; `TIMER_PHONE` 48 and ids 49-50.


## Integration (Run B finish)

Run B's four features were built in parallel (clip-drop and send-phone on `main`,
stacks and tuck on `task/sticky-v2b-stack-tuck`) and merged here with real merge
commits (never a rebase). The rules where they meet, all decided from the merged
code and covered by host or apply tests:

- **Window setup.** `sync` makes one window per unit (a stack is one window,
  `stack::match_units`). After every `create_window()` that returns a window,
  `drop::register(hwnd)` runs at once, so every unit window is a drop target; a
  window that is only retargeted (a flip, a join, a split that keeps it) keeps its
  single registration (registered once per HWND, never twice). `WM_DESTROY` calls
  `drop::revoke` first, and every way a unit window goes (a sheet leaving, the last
  sheet gone, `shutdown`) is a `DestroyWindow`, so each passes there. Test:
  `the_drop_target_is_registered_once_per_window_and_a_stack_change_keeps_it`
  (count 1, then 2 after a sheet is taken out, then 0 after shutdown).
  `blank_win` sets `drop_hover: false` next to the stack and tuck fields.
- **Drop + stack = the shown sheet.** The drop target works by HWND, and
  `drop_into` reads the window's own `id` / `text`, which a flip swaps
  (`show_sheet`), so the lines land in the sheet the window shows, never the
  stack's first or bottom one; the `Text` event carries that sheet's id. The
  dashed outline follows the visible shape: the top sheet's rectangle (the paper
  edges under it are not outlined), the note itself while a tucked note is out.
  Test: `a_drop_on_a_stack_lands_in_the_shown_sheet_not_the_first` (flip to sheet 3,
  drop, apply: only sheet 3 changed, and flipping back and forth keeps it).
- **Drop + tuck: the note comes out.** A tab alone is only 16 dip wide and the rest
  of the window is click-through, so a drag can only reach a tucked note over its
  tab. `drop_hover(hwnd, true)` (the drop target's `DragEnter`) is now part of
  `keep_out`, like the pointer, the keyboard, a drag or a popover: the note slides
  out under the drag (the same slide on the frame clock, instant without
  animation), and the drop then appends to that note. `drop_hover(hwnd, false)`
  (`DragLeave` and `Drop` both call it) runs `tuck_settle`, which starts the usual
  300 ms grace and the slide back in. Nothing can stay stuck out: leaving, dropping
  or the window going all clear it, and **no timer was added** (the grace counts on
  the existing frame clock). Tests:
  `a_drop_on_a_tucked_note_brings_it_out_and_appends_to_that_note` (animate on
  and off, drop, leave, back to a tab, clock off) and
  `a_drag_that_leaves_a_tucked_note_without_a_drop_lets_it_slide_back`.
- **Send to phone on a stack.** "Kirim ke HP" (id 26) is the `send_to_phone(hwnd)`
  helper, which pushes `SendToPhone` with the window's current `id`: the sheet it
  shows after flips. Test: `send_to_phone_on_a_stack_sends_the_sheet_the_window_shows`
  (first sheet, then after a flip to sheet 2). The island then snapshots that note
  (`apply` -> `Outcome::send_phone`), unchanged from [[shelf-phone]]'s page.
- **The cap counts sheets, prefill included.** `NotesStore::sticky_count` counts
  notes that have a sticky part, and every sheet of a stack is one, so a stack of 3
  is 3 and the host's `sheet_total` agrees with it. Every way to make a note asks
  `can_add(sticky_count())`: the gallery (+), the tray, hotkey 7, the header (+),
  the Notes page pin, and the clipboard prefill. `new_sticky_with` checks the cap
  before anything and shows the "sticky-full" notice; a prefilled note is one sheet
  however many lines it has; `cliptext::fill_wanted(on, sheets)` (on and below the
  cap) decides whether the clipboard is read at all, so at the cap it is not read.
  Tests: `at_twenty_sheets_with_stacks_every_way_to_make_a_note_makes_nothing`,
  `a_prefilled_note_counts_as_one_sheet_like_any_other` (apply.rs) and the host's
  `the_cap_counts_every_sheet_of_every_stack`. Not unit-tested, so a manual check:
  each of the five entry points at 20 sheets really shows the notice on the
  island (the `App` methods need the island window).
- **Menu ids.** `CMD_UNSTACK` 24, `CMD_TUCK` 25, `CMD_SEND_PHONE` 26, all distinct
  and outside the paper (1..), remind (30..), stamp (50..) ranges
  (`the_menu_ids_are_distinct_and_the_documented_ones`). The menu keeps both
  sides' items: Cap, Kirim ke HP, Pin, Roll, Keluarkan dari tumpukan (stacks only),
  Selipkan / Keluarkan (tuck), Unstick, Delete.
- **Demo.** `--demo-widgets reset` seeds sixteen notes: the twelve of main (eleven
  of run A and the clipboard sample note, Krem, from the fixed sample string, never
  the real clipboard, stamped `now - 40`), the three "Proyek Dago" sheets
  (`now - 30 .. now - 28`) and the note tucked on the right (`now - 20`). 16 stays
  under the cap of 20; `the_demo_seeds_sixteen_notes_with_the_clip_note_a_stack_and_a_tuck`.
- **Shared slots.** No new `TIMER_*`, `WM_APP` or hotkey id: widgets use timers 45,
  46 (the host's caret) and 47; 48-50 are the phone's; `WM_APP` +60 PrtSc, +61
  Arrange, +70 lyrics card, +71 the host's own message; hotkey ids 1-7.
- **Run A's drag rules are intact** (the 36 dip strip drags, hidden buttons take no
  press, Alt + drag anywhere, a just-created note drags at once): their tests pass
  unchanged. Shift + drag onto a note makes a stack only with Shift held; a plain
  drag still just moves and never stacks (`a_plain_drag_over_another_note_does_not_stack`).

### Verified and not verified (Run B finish)

Verified: `cargo test --locked` green (2791 passed, 16 ignored, the same 16 as on
main), `cargo build --release --locked` with no warnings.
Not verified live (manual checks): a real OLE drag of text, a link and a file from
another app onto a stack's top sheet and onto a tucked note's tab (the unit tests
call `drop_hover` and `drop_into` as the drop target does); the island notice at 20
sheets from each entry point; Send to phone from a flipped stack with a real phone.
