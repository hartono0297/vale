# Sticky note peek follows the chosen mascot

Visual: `docs/design/sticky-peek-maskot.html` (approved 2026-11-27).

## Intent

The mascot that peeks over a sticky note (holds the clock on an overdue
reminder, smiles when the checklist is done, sleeps on a note untouched for
three days, looks toward the pointer) is always the cat. It should be the
mascot the user picked in Settings -> Maskot (cat, falk/cockatiel, octopus),
in that mascot's own coat.

## Cause

`app_widgets.rs` hands the host only `coat: config.cat_color`, and
`paint::peek_look(coat)` builds a `CatLook` with the default mascot (Cat).
`RobotPainter::paint` already draws every mascot from a full `CatLook`.

## Design

- The widget `Ctx` carries the whole look (mascot, cat colour, falk colour,
  octopus colour) instead of the cat colour alone; the host keeps it and
  `peek_look` takes it and only drops the accessory (a hat would be cut by
  the paper's edge, as today).
- Place, size (44 dip), clip (30 dip room, 47 of 64 rows above the paper),
  clock, "z", moods and their order do not change. Every mascot sits at the
  same height; the falk's beak stays under the paper (owner's call accepted).
- Changing the mascot or its coat in Settings redraws the notes (the
  existing config-adopted sync path).

## Approaches

Only one sensible way: pass the full look through the existing sync. A
second peek painter per mascot would duplicate `RobotPainter`; a global read
of the config from the widget host would break the host's pure `Ctx` input.

## Non-goals

No new setting, no per-note mascot, no accessory on the note, no geometry
change. Settings files (being reworked in a parallel session) are not
touched.

## Files

`src/widgets/host.rs`, `src/widgets/paint.rs`, `src/app_widgets.rs`,
`src/widgets/demo.rs` (if it builds a `Ctx`).
