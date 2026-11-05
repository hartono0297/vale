---
tags: [feature, widgets, sticky-notes, win32, hotkeys, power]
date: 2026-11-05
---

# The sticky note list ("Daftar catatan tempel")

One window that lists every sticky note, with a search box, so a note that is
tucked, rolled up, hidden in a stack or buried under other windows can be found
and brought to the front. Part of [[widgets]]; the hotkey row is in [[hotkeys]];
the window follows the Settings window palette ([[terminal-windows]]); the
power rules are in [[bata-idle-power-budget]].

## Three ways to open it, one window

- the **list button** in a note's header (`Btn::List`, a hamburger, the leftmost
  of seven): the window opens beside that note (right of it, else left);
- the tray item **"Daftar catatan tempel"** (`MenuCmd::StickyList`, right after
  "Catatan tempel baru", always enabled);
- the **eighth hotkey row** (`HotkeyId::StickyList`, config key `sticky_list`,
  no default, `RegisterHotKey` id 8).

The tray and the hotkey open it at the top right of the notch's monitor work
area. A second open while it is open moves the one window to the new anchor,
refreshes it and focuses it (`list_win::open`): never two windows.

## Header buttons: BTN 22 -> 20, BTN_GAP 2 -> 1

Seven 22 dip buttons do not fit the smallest note (`sticky::MIN_W` 160): 7 x 22 +
6 x 2 = 166. Now it is 7 x 20 + 6 x 1 = 146, so the row spans x 8 .. 154 at 160
dip and starts at 108 at `stack::MIN_W` 260, where a stack's arrows end at 96
(tested at every stack width from 260 to 640: the Next arrow ends before
`Btn::List`). The date still ends at `button_rect(Btn::Pin).x`. This 2 dip
smaller button is the only change to how an existing note looks. A shown List
button hits `Target::Button(Btn::List)`, a hidden one is `Target::Header` (the
strip drags), like the other six.

## Code map

- `src/widgets/list.rs`: **pure, no Win32, tested.** `rows` (one `Row` per
  sticky note, every stack sheet has its own with its 1-based place from
  `stack::units`; a stack id one live sheet carries alone is no stack), the
  title (`checklist::title_of`), the snippet (lines after the first, checklist
  markers removed, blank lines dropped, joined with " · ", whitespace collapsed,
  capped at 120 chars on a char boundary), the reminder (`Some(overdue)` when a
  reminder is set and unfired or overdue), progress, sort by `updated` desc then
  id; `filter` (trimmed, `to_lowercase` on both sides, title or whole body);
  `layout` / `hit` / `row_at` / scroll / `wheel` (one notch is one row) /
  `ensure_visible`; `row_geo` (the parts of one row, with a third line only for
  a stack badge or a checklist count); `place_beside` / `place_default` (px,
  always inside the work area); `move_selection` and friends; `show_events`;
  `Undo`.
- `src/widgets/list_win.rs`: the `ValeNoteList` layered popup (`WS_POPUP`,
  `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`), modelled on `pop.rs`.
  `open`, `refresh`, `restyle`, `is_open`, `close`, `drain`; events
  `ListEvent { Show, Delete, Undo, New, Closed }`, posted to the owner as
  `host::WM_APP_WIDGET` once until drained (own queue, so `Closed` outlives the
  window).
- `src/app_note_list.rs` (child of `app`): `open_note_list`,
  `note_list_events`, `note_list_refresh`; `WidgetsState.list_undo`.
- `i18n.rs`: `Key::StickyList*`, `count_label` ("7 catatan" / "7 notes",
  "1 note"), `stack_badge` ("Tumpukan 2/3" / "Stack 2/3").
- `notes.rs`: `NotesStore::restore` (keeps the id, `place_by_time`; refused for
  an existing id or a desktop note at the cap). **No new JSON key anywhere**: a
  delete + undo round trip writes the same file (tested).

## The window

340 x 460 dip times the DPI of the monitor it lands on (`GetDpiForMonitor`; a
short work area shrinks the height, minimum 240). `WM_DPICHANGED` re-scales and
keeps the top-left. The header outside its buttons answers `WM_NCHITTEST` with
`HTCAPTION`, so it drags (maximise, the system menu and Alt+Space are swallowed);
the position is never saved. Colours come from `theme::current_window()` (so
light, dark and Terminal follow Settings and the popovers); the only paper colour
is the row's 4 dip left strip, `sticky::tones(paper, island, accent).fill` as the
host computes it. The font is the UI font option (`Res`). Title in 600 weight,
snippet in the secondary ink, one line each with DirectWrite ellipsis trimming.
The right column is the time (`notes::note_time_label`) and under it small
status icons (`ui/icons.rs`: Pin, ChevU for rolled, Ext for tucked, Clock or a
Bell in the accent for a reminder); a third line carries the stack badge (Copy
icon + "Tumpukan 2/3") and the checklist count (Check + "2/4", accent when all
done).

It closes on Esc, on its own close button and on `WM_ACTIVATE` `WA_INACTIVE`
(**posted** as `WM_CLOSE`, not done inside the handler; ignored while the native
row menu is up). Showing a note, making a new one and clicking elsewhere all end
in that deactivation.

## Search through the hidden EDIT

The same pattern as the sticky note's keyboard ([[widgets]], `edit.rs`): a
layered window never draws its children, so the list draws the text, the caret
and the selection itself (DirectWrite `HitTestTextPosition` / `HitTestTextRange`
/ `HitTestPoint`, surrogate-safe with `paint::hit_index` / `settle_index`, now
`pub(crate)`), and a one-line child EDIT, answering `WM_NCHITTEST` with
`HTTRANSPARENT`, gives the IME (placed at the caret), paste, cut and undo. The
caret is steady: no blink timer. It has the keyboard when the window opens and
again after the row menu. `EN_CHANGE` (and every key through the subclass)
re-reads the text; a new text filters, resets the selection and the scroll to 0.
Typing "Cari catatan..." is the placeholder.

The subclass forwards: **Esc** closes, **Up / Down / PageUp / PageDown** move the
selection (and keep it in view), **Enter** opens the selected row or, with none
selected, the first visible one, and the wheel scrolls.

**Delete key rule:** Delete deletes a row only when a row is selected with the
keys; otherwise it stays the EDIT's (it deletes a character of the query).
Typing clears the selection, so Delete goes back to the EDIT. After a delete the
selection moves to the row that takes its place.

Mouse: left click on a row shows it, a right click opens a native
`TrackPopupMenu` ("Buka", "Hapus"), the wheel scrolls (one notch is one row),
hover rides `WM_MOUSEMOVE` + `TrackMouseEvent(TME_LEAVE)` of this window only.
(+) and the "Buat catatan" button of the empty state both push `New`; at the cap
they are greyed and the header's count line carries the reason ("Maksimal 20
catatan tempel"). Empty states: "Belum ada catatan tempel" with the button;
"Tidak ada yang cocok" for a search without a match.

## Show and focus

A click, Enter or "Buka" pushes `Show(id)`. The owner (`note_list_show`):
`widgets_reveal()`, applies `list::show_events` (`Tuck { edge: None }` for a
tucked note, `Rolled { on: false }` for a rolled one, `StackShow` for a hidden
sheet; nothing for what is already so) through `apply::apply`, arms the save,
`widgets_sync()`, then `WidgetHost::bring_front(id)` and closes the list.

`bring_front` is `focus(id)` plus `Win::lifted`: a flag that makes
`above_windows` true, so `apply_z` puts a note of the desktop layer over other
windows (topmost, no shell owner). `end_editing` (the note loses the keyboard)
clears it and runs `apply_z` again, so the note goes back to its layer. The flag
is set only if the note did get the EDIT, so nothing can leave it stuck.

## Undo snapshot rules

`Undo { note, mates }` is taken **before** the delete (`Undo::capture`): the
note exactly as it is, and the sticky part of every other sheet of its stack.
`Undo::restore(store)` puts the note back with its own id, text, `updated` and
sticky part (`NotesStore::restore`; refused when the id exists or the desktop is
at `MAX_STICKY`). For each mate still on the desktop it writes back **only**
`stack`, `stack_order` and `stack_top`, so a paper or a stamp changed in the
meantime stays. When the note rejoins a stack it takes that stack's *current*
shared fields (`stack::share`: rect, monitor, pin, roll, tuck), so the stack is
one window again. The caller runs `apply::settle`. If every mate is gone,
`settle` clears the lone stack id. Tests cover a plain note, each sheet of a
3-sheet stack (identical file after delete + undo), a mate changed meanwhile, a
missing mate, an existing id and a full desktop.

The strip ("Catatan dihapus · Urungkan", 40 dip at the bottom, shrinks the row
viewport) is owned by the window: a delete shows it for 5 s. `TIMER_LIST_UNDO`
= 49 is a **one-shot on the list's own window**, set only while the strip shows
(a second delete restarts it), killed when it hides (time up, "Urungkan"
clicked, window closed). Time up just hides the strip: the delete stands. The
owner keeps the snapshot until the next delete or `Closed`. "Urungkan" is greyed
when the desktop is full (`can_undo`). Closing the window pushes `Closed`
(commits: `list_undo` dropped).

## Power

Nothing while it is closed: no timer, hook, poll or pointer tracking. The data
(a `Vec<Note>` clone) is built only while it is open; `widgets_sync`,
`widgets_apply_list` and the delete/undo paths call `list_win::refresh` only
when `is_open()`. Open, it reacts to messages only; no FrameClock (an instant
hover). Timer 49 only while the strip shows. Notes-page edits and deletes
already end in `widgets_sync`, so no extra hook was needed after `persist_notes`.
`app::tests::timer_ids_never_collide` lists 49.

## `--demo-note-list [reset]` (debug)

The same scene, notes file (`%TEMP%\vale-widgets-demo\notes.json`) and 16
seeded notes as `--demo-widgets` (mixed papers, a tucked note, a rolled one, a
pinned Gelap one, a 3-sheet stack, a reminder and an overdue one, checklists),
with the list opened at the default anchor once the windows are up. The demo
owner handles the `ListEvent`s itself with the same pure functions
(`show_events`, `Undo`) and the host's `bring_front`. Never real user data,
never the real clipboard.

## Gotcha: the search EDIT painted a white box

The first version made the one-line EDIT as wide as the search box. On screen
(`CopyFromScreen` of the real desktop, not only `PrintWindow`) it showed as a
white rectangle over the layered bitmap: a child of a layered window **can**
paint over what `UpdateLayeredWindow` put there. Fixed the way the CLI page's
input does it plus one more step: the EDIT is **1 x 1 px** at the text origin
(the IME's composition window is placed from there), and its subclass answers
`WM_PAINT` (`ValidateRect`), `WM_ERASEBKGND` and `WM_NCPAINT` without drawing and
destroys the caret the EDIT makes on `WM_SETFOCUS`. It is also created after the
window is shown and drawn once. Test: `the_search_edit_is_transparent_to_the_pointer_and_takes_the_query`
covers the hit-test answer; the look was checked on screen.

## Verified and not verified

Verified (run in this session):

- `cargo test --locked`: 2872 passed, 16 ignored (the same 16 as on `main`), and
  `cargo build --release --locked` with no warning. New tests: `list.rs` (title and
  snippet with markers and emoji, the cap on a char boundary, sort, filter case and
  Unicode, every status flag, the stack badge and a lone stack id, layout and hit
  geometry, scroll and wheel, `ensure_visible`, placement right / left / clamped on
  two monitors and at 150 %, selection, `Undo` for a plain note and for each sheet
  of a 3-sheet stack, identical file after delete + undo, a changed or missing mate,
  an existing id, a full desktop, `show_events` and applying them), `list_win.rs`
  (17 tests on a real hidden window: one window only, ex-style, filtering, the keys,
  the Delete rule, the strip, Urungkan greyed at the cap, clicks, the empty-state
  button, refresh and a gone selection, `Closed`, the posted close, no close while a
  menu is up, `HTCAPTION` only on the header, one posted message per batch), the
  header buttons (seven fit at 160, 240, 260 and 640; Next ends before List at every
  stack width; a hidden List button is the strip), `host.rs` (`OpenList`,
  `note_rect_px`, `bring_front` and `lifted`), the hotkey id and table, the tray item
  in both languages, `NotesStore::restore`, `count_label` and the stack badge,
  Settings with eight hotkey rows.
- The debug `--demo-note-list` on this PC, in an isolated temp folder (`TEMP` and
  `TMP` redirected), looked at with `CopyFromScreen` and driven with window messages
  (not a real mouse or keyboard): the list in Indonesian, English and the light window
  theme (rows with papers, the tucked note's icon, stack badges, checklist counts,
  ellipsis, the focus ring and the steady caret); typing "dago" filters to the three
  sheets; two Down and Delete remove "Dago: struktur" (the stack is 2/2, the strip
  "Catatan dihapus · Urungkan" shows, the pick moves to the next row); a click on
  Urungkan brings it back as sheet 2/3; searching "tulisan" and Enter closes the list,
  and the foreground window is that note, topmost, drawn over a browser window with the
  caret in it.

Not verified live: a real mouse drag of the header (`HTCAPTION`, the modal move loop),
a real IME (the composition window's place), the native row menu with a real right
click, the wheel from a real wheel, `WM_DPICHANGED` and a second monitor or 150 % scale
(only the placement maths is tested), the real island's three entry points (the tray
item, the hotkey, the header button; the demo owner and the island share the pure
functions and `bring_front`, but `app_note_list.rs` itself has no test since `App` needs
the island window), Show on a tucked or rolled note on screen (the events and their
application are tested, and the host retargets by the same `sync`), and the list
closing by deactivation while the machine is busy: on a heavily loaded PC the first
demo launches opened no list window at all, which a quiet re-run did not repeat (the
likely cause is the foreground changing hands during start-up, which closes the list
by design). Also: the demo run reset `%TEMP%\vale-widgets-demo\notes.json` once by
mistake (TMP was not redirected), which only reseeds `--demo-widgets`.

## Images and the undo (2026-11-05)

A deleted note's image files stay on disk while "Urungkan" can still bring the
note back: `App::widgets_images_release` keeps the images of `list_undo` and
deletes the rest of what is no longer referenced after every notes save. The undo
ends (and the release runs again) on `ListEvent::Closed` and on the new
`ListEvent::UndoExpired`, which the list window pushes when its 5 s timer 49
fires; the owner then drops the snapshot too (`list_undo = None`). See [[widgets]],
"Format toolbar and images".

