---
tags: [decision, panel, settings, v3.4]
date: 2026-11-01
---

# Panel pages: order and which ones the wheel shows

Run 9 of the v3.1 plan. Settings → Panel (was Tampilan before v3.8) → "Halaman panel" lists all 13
pages (every page in `Page::ALL`) in the owner's order, each with an order
stepper (− earlier, + later) and a toggle. `src/pages.rs` (`PanelPages`) is the pure model.

- **Every page is always listed once.** `config.json` keeps `panel_pages` as
  `[{page, on}]`; unknown and repeated names are dropped, a page the file
  does not name (an older config) joins right after its default neighbour,
  and a list with every page off shows them all. A bad value never throws the
  rest of the file away (lenient deserializer).
- **Off means skipped, not gone.** The wheel and the page dots only count the
  pages that are on, but a page's own button still opens it (the Clipboard
  tile, a disk warning's "Buka"); on such a page no dot is lit and the wheel
  moves to the nearest page that is on.
- **The last page shown cannot be switched off**: its toggle is disabled.
- **A page switched off while it shows** hands over to the next page on (or
  the one before it at the end of the order).
- **Slides follow the owner's order**, and the page drawn beside the new one
  is the page it came from (`Frame::page_from`), not a neighbour in
  `Page::ALL`.
- The card sits in Settings → **Panel** (since v3.8, under "Perilaku": Mode
  buka and Di belakang jendela; it was the end of Tampilan, [[settings-structure]]).

- A page that is off but opened by its button holds only for that visit:
  when the panel closes the page moves to the nearest one on, so the next
  hover does not land on it.

Related: [[settings-appearance-scroll]], [[v3-studio-overview]]

**v3.5:** `Page::Devices` sits between System and CLI, so `Page::ALL` is 12
pages ([[devices]]); a saved list without it gets it back right after System
(`from_saved`, tested).

**v3.9:** `Page::Notifications` ("Notifikasi", [[notification-history]]) is the
13th page, directly after Clipboard and on by default (`Page::ALL: [Page; 13]`,
name `"Notifications"` in `config.json`). A saved list without it (an older
config) gets it right after Clipboard wherever the owner moved Clipboard to
(`from_saved`, tested), shown. Its jump tile `Shortcut::Go(Page::Notifications)`
sits after Mixer's in `Shortcut::ALL` (21 built-ins) and is **off by default**
(an old config joins it off, the twelve-tile page keeps its twelve). Icon:
`Icon::Inbox`, an inbox tray with a notch, not the Bell of Reminders.

## The Settings card, remade (2026-10-29)

The "− n +" order stepper was confusing and is gone. Settings → Panel →
Halaman panel now reads:

- **Shown pages first**, in order: a drag handle at the left, up/down
  buttons (`PanelPages::move_shown`: among the pages shown, stepping over
  hidden ones) and *Sembunyikan*. Under a small "Tersembunyi" title come the
  hidden pages, each with *Tampilkan*. The header says "Tampil di panel:
  n/13" and holds *Kembalikan bawaan* (`PanelPages::default()`; off while
  the list already is the default).
- **The last page shown cannot be hidden:** its button is disabled and the
  row says "Minimal satu halaman tampil".
- **`PanelPages::move_to(page, index)`** takes a page out and puts it at a
  place of the whole order (hidden pages count); a drag drop maps "place
  among the shown" to the entry index of the page it lands on
  (`settings::page_drop`). `move_by` is gone.
- The drag, the raised row and Esc to cancel are shared with the Pintasan
  card, see [[shortcut-tiles]]. A Shortcuts tile can now open any page,
  including a hidden one (for that visit only, as the Clipboard tile).

A sideways two-finger swipe over the open panel follows the same order and
skips as the vertical wheel (`Event::PageFlick`, see [[gestures]]).

**2026-11-01:** The pages card shows each page's Dock icon in its row
(`shortcuts::page_icon`, the one page-to-icon mapping, also used by
`paint_page_dots`): 18 dip, secondary ink, at `card.x + 34`; hidden rows
("Tersembunyi") draw it dimmed (0.45 alpha). Names start at `card.x + 62` in
both groups so they line up. The icon is a `Layout.glyphs` item (`Glyph`): a
decorative rect with no hit region and no focus stop, shifted by the scroll,
painted in `cards_and_controls` after the lifted plate (so the held row's icon
rides the raised plate) and before the texts.

## Page dots become a dock (hover)

Hovering the page dots under the open panel turns them into a row of page
icons with macOS-Dock magnification. Pure model in `src/ui/dock.rs` (tested),
the pointer and the frame step in `src/app_dock.rs`, drawing in
`render_panel::paint_page_dots`, the demo in `dock-hover` and
`dock-hover-edge` (`DI_DOCK_X=<dip>` moves the pointer).

- **Zone.** The dots' bounds grown by 10 dip vertically and 8 horizontally.
  While expanded, the zone is the resting icon row's bounds plus the same
  padding, reaching up over the click cells (hysteresis: a peak icon and the
  top of its cell stand above the padded row, so the padding alone dropped a
  pointer on a magnified icon's top; tested). Leaving the zone,
  `WM_MOUSELEAVE`, or a page with no dots (cleaner, Mole, search: the
  renderer registers no `DockZone`) eases the row back to dots; the panel
  closing resets it at once (`App::dock_sync`, before every draw).
- **The zone stands through slides.** `Renderer::draw` resets the regions
  every frame, and the click regions wait for a settled page
  (`panel_regions_wanted`). The `DockZone` must not wait with them: with no
  zone, `dock_sync` collapsed the row the moment its own click (or a wheel
  or flick) started a slide. `dock::panel_parts` builds the zone whenever the
  page has dots (not while closing) and the icons' click regions only when
  the page takes clicks.
- **Resting layout** (`resting_layout`, never moves): `DOCK_ICON` 14 dip at a
  `DOCK_PITCH` of 28 dip, centred like the dots, bottom edge on the dots'
  line (`s.y + s.h - DOT_BOTTOM`). One icon per page of `frame.pages` (the
  owner's order, `PanelPages::visible`), art from `shortcuts::page_icon`; the
  active page in the accent colour, the others secondary. A backdrop pill in
  the surface colour sits behind the row so icons growing over the content
  stay legible. Falloff and click rects read this layout only, so an icon
  growing under the pointer never shifts its own target (no shake).
- **Falloff.** Target scale `1 + 0.9 * falloff(|pointer_x - centre|)`,
  `falloff(d) = 0.5 * (1 + cos(pi d / 56))` below 56 dip, else 0. Peak 1.9x,
  the icon grows from its bottom edge. Pitch 28 with reach 56 keeps
  neighbours from overlapping (tested).
- **Smoothing.** Mouse moves only set targets. The shown values (each scale
  and the expand progress `e`) advance once per `FrameClock` frame by
  `v += (target - v) * (1 - exp(-dt / 0.07))`: frame-rate independent, no
  overshoot, snaps within 0.002. During the morph each dot's rect lerps to
  its resting icon's rect by `e` (centre, bottom and size,
  `dock::morph_rect`), magnified by its scale, and dot and icon crossfade in
  that rect. With Windows animations off
  (`App.animations`) the values snap.
- **Click.** `Hit::PanelPage(page)`, active only while `e > 0.5`. Each icon
  owns a pitch-wide cell from the baseline up to the grown height (so what
  is seen is what is hit; hit-testing never reads the scales). The current
  page's cell is a region too (`Hit::PanelPage(current)`, which `go_to_page`
  ignores and which arms no linger), so a click on its magnified icon never
  falls through to the page content under it. In `state.rs` `go_to_page` follows the flick's rules
  (vertical residue zeroed, the sticky-reminder peek spends the click,
  nothing on the search or a closed panel) and the linger arms as a page
  turn (`LingerTrigger::PageTurn`, so the current page arms nothing). The
  slide direction is the pages' places in the owner's order, as ever.
- **Power.** No timer: see [[bata-idle-power-budget]]. `App::animating_at`
  includes `dock.moving()`; a pointer resting on an icon stops the clock.

Related: [[panel-linger-and-motion]], [[gestures]]

**v3.10:** `Page::Arrange` ("Susun", [[arrange]]) is the 14th page, between Cli and
Shortcuts (`Page::ALL: [Page; 14]`, name `"Arrange"` in `config.json`). A saved list
without it (an older config) gets it right after Cli wherever the owner moved Cli to
(`from_saved`, tested), shown. Icon: `Icon::Layout`, a screen frame cut into a 2x2
layout, not Pintasan's `Grid`.

## The header Settings button (2026-11-02)

Every open-panel page carries a fixed Settings button (`Icon::Sliders`) at the
top right, in the header row. It opens Vale's Settings window.

- **Helpers** (`panel_layout.rs`): `settings_button(s, strip)` is one 22 dip
  square at `right(s) - 22`, `content_top(s, strip)`, the same rect on every
  page so it never jumps between pages. `header_right(s)` (`right - 22 - 8`) is
  the right edge of every header-row control; each layout's right-aligned
  header part (calendar selector, Pomodoro durations, "+ Baru", CLI expand and
  its chips, Clipboard search, Notifikasi clear and pause, Search field,
  Shortcuts and Mixer header text, Shelf clear, System clean, cleaner and Mole
  segment, Devices eject all, Susun skip and chips) ends there. Body content
  below the header row keeps `right(s)`. The Media page has no header row: its
  text column and "Lirik" give way only where they would reach the button (a
  guard that does nothing at the default panel).
- **Hit.** `Hit::OpenSettings` is the first region of `regions()` for every
  page (right after `Hit::Strip`), so it wins over underlays such as
  `NotifFocus` and `CliMenuDismiss`. The reducer answers with
  `collapse_for(Effect::OpenSettings, ..)`: only on an open panel, it collapses
  (which ends the linger) and `App` opens Settings after the handler returns
  (`DEFERRED_OPEN_SETTINGS`, see [[shortcut-tiles]]).
- **Drawing.** Fixed chrome like the page dots, painted in
  `paint_panel_content` outside `paint_page_at` (no slide, cross-fade or reveal
  offset), also while closing, and offset by `strip_dy` while a toast strip
  moves. It fades with the panel through the existing masks.
- **No hover plate, by design.** The panel tracks no pointer over header
  buttons (the calendar's arrows draw the icon only), so the button does
  likewise; no timer, tooltip, focus stop or hotkey was added, and nothing runs
  at idle ([[bata-idle-power-budget]]).
- **Tests.** `settings_button_is_clear_of_every_page` checks the rect against
  every page's regions and drawn rects (`drawn_rects`, shared with
  `drawn_rects_inside_panel`), strip on and off, both styles.
