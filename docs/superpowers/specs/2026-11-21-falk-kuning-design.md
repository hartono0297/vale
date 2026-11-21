# Falk "Kuning" and per-mascot eyes on the site

Date: 2026-11-21. Visuals shown to the owner: `falk-abu-*.html` and
`falk-abu-animasi-v3.html` (session scratchpad, not committed).

## Why

1. The falk's second coat ("Abu-abu" / Grey) reads as a bearded bird: the head
   is a grey egg with a yellow face mask that stops above the chin, so a grey
   band shows under the orange cheeks (owner: "kyk ada jengot"). The owner
   tried bottom colours, top colours and mask shapes, rejected a yellow head
   on a grey body ("aneh"), and chose: the whole bird yellow, renamed
   **Kuning** (Yellow).
2. On the landing page the octopus's and the falk's eyes sit too low and too
   big (owner: "kenapa mata octopus dan burungnya begitu"). Root cause, found
   by measuring the live page: the hero has one eye group drawn for the cat
   (logo geometry, x 78/152, y 147.5, 26 x 46 in the 256 grid), shared by all
   mascots and only lifted 12 for the octopus; the falk and octopus bodies are
   the app's 64 grid times 4, whose eyes are pills 22.4 x 40 at x 84.8/148.8,
   y 140 (octopus y 128). On top, the pointer glance moves the eyes up to 11
   down, a range sized for the cat, which pushes them onto the octopus's arms.

## Goals

- Coat 2 of the falk is yellow all over in the app and on the site: head egg,
  face, crest and cheeks as now, body, belly, wings and tail yellow.
- It is called "Kuning" (ID) / "Yellow" (EN) everywhere it is named.
- The hero's falk and octopus eyes (open, happy, squint) sit where the app
  draws them, at the app's size, and the glance never takes them off the face.

## Non-goals and anti-goals

- Lutino and Muka putih (WhiteFace) do not change at all (owner: "yang
  sisanya jangan diubah").
- No new coat, no new setting, no release.
- Failure even if it works: Kuning becoming hard to tell from Lutino in the
  Settings swatch or the notch; a stored `falk_color` changing for anyone.

## Decisions

- **App colours:** `falk_ink(FalkColor::Grey)` becomes body `0xF2D35A` (the
  face colour), face `0xF2D35A`, crest `0xE9CF62`, cheek `0xEC6B3A`. The head
  egg is drawn in `body`, so the band disappears; the wings, belly and tail
  come from `falk_coat`'s `tone(body, ...)`, so they turn yellow with no new
  table. Grey and WhiteFace no longer share one wing (the test that says so
  changes).
- **Notch tint** (`notch_colour::tint`): Grey becomes `0xF2D35A`, like the
  face; the 5:1 cream and 4.5:1 caption rules of `bottom()` still hold (tests).
- **Surface** stays Night for this coat (Lutino keeps Espresso), so the two
  yellow birds still differ in the notch. Owner may change this later.
- **Name:** the config value stays `Grey` (`named_enum!`, stored in every
  existing `config.json`; renaming would reset users to Lutino). Only the
  label changes: `Key::FalkColorGrey` reads "Kuning" / "Yellow". A comment at
  the enum says why the variant name and the label differ.
- **Swatch:** the Settings coat swatch for this coat must stay tellable from
  Lutino's (both yellow): show its Night surface behind it, or a two-tone
  swatch, whichever the Settings code already supports; the builder picks and
  shows it.
- **Site:** `MASCOTS.falk.coats.grey` label "Yellow", colours as above, the
  sprite and hero falk follow; key `grey` stays (stored `vale-coat-falk`).
- **Eyes on the site:** each mascot gets its own eye box in the hero svg:
  cat unchanged; falk pills x 84.8 and 148.8, y 140, 22.4 x 40, rx 11.2;
  octopus the same 12 higher (y 128). Happy and squint marks move and scale
  with them. The glance range for falk and octopus is scaled to their face
  (x and y times 22.4/26) and clamped so the pills stay inside the face:
  falk above the cheeks' centre line, octopus above the mantle's bottom with
  at least 2 grid units (8 in the 256 grid) to spare.

## Done means

- `cargo test --locked` green, with tests for the new Kuning ink, the notch
  tint rules, the WhiteFace wing unchanged, the label.
- `scripts/check-site.py` 0 problems; a browser check on the live-like
  local site: the eyes of falk and octopus inside their faces with the
  pointer at every edge of the viewport; Kuning in the hero, the notch and
  the coat chips; Lutino and Muka putih pixel-identical to before.
- Demo modes (`--demo`) show Kuning; the owner checks the app by eye.
- Merged into `main`, pushed, memory updated (maskot-falk-gurita, site-landing).
