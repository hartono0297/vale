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
on `WM_DESTROY` / `WM_ENDSESSION`, cleared by any notes write), 46 and 47 (the
reminders' one-shot, run A below). App
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
  cat's face (`art::deco_centre`, tested; the room is kept whether or not the
  cat is there, so they do not jump).
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
