---
tags: [decision, mascot, cat, painter, settings, geometry, power]
date: 2026-11-13
---

# Two more notch mascots: the falk and the sitting octopus

Owner request (2026-11-12): a mascot besides the cat, a bird or an octopus.
After four mockup rounds (`docs/design/maskot-falk-gurita-animasi.html`,
`maskot-falk-sayap.html`, `maskot-pilihan-akhir.html`) the owner chose both:
the **falk** (cockatiel, from a photo of the owner's own bird; "Falk A,
pilihan 1") and the **gurita duduk** (sitting octopus). Spec:
`docs/superpowers/specs/2026-11-12-maskot-falk-gurita-design.md`, plan:
`docs/superpowers/plans/2026-11-12-maskot-falk-gurita.md`.

## What the owner rejected on the way

- Bird round 1: a round yellow bird with a tuft and stubby wings read as a
  chick; an owl, a cockatoo and a green songbird came next, then the owner's
  own cockatiel photo.
- Octopus round 1: a dome with arms hanging straight down read as a
  jellyfish. The kept one has a balloon mantle narrowing down, the eyes low on
  it, six arms spread sideways on the floor with curled tips and suckers.
- Falk with folded wings (they read as headphones, then as tusks) and with a
  tail (looked like a stand): the falk is head only, like the cat.

## How it is built

- Config: `mascot` (Cat, Falk, Octopus; default Cat), `falk_color` (Lutino,
  Grey, WhiteFace), `octopus_color` (Coral, Red, Purple, Blue), lenient, at
  the end of `Config`, island-owned. `cat_color` and `cat_accessory` keep their
  keys; each mascot keeps its own coat, the accessory is shared.
- `theme::Coat` (`from_parts`, `of`, `all_of`) is the one mapping;
  `Surface::resolve(coat, robot_on)`: Falk Lutino → Espresso, Grey → Night,
  WhiteFace → Plum; Octopus Coral → Espresso, Red → Graphite, Purple → Night,
  Blue → Sea. Auto never gives Terminal.
- Geometry and painters: `src/ui/mascot.rs` (pure data, the `impl
  RobotPainter` block for the two bodies, `mascot_box` for the see-through
  halo). `RobotPainter::paint` keeps one path: rim, calico and paws are the
  cat's only; the new bodies replace steps 1 to 3; face marks, accessories and
  the laptop are shared. Eye marks are espresso on every new coat (no amber,
  no rim). The octopus's eyes and glasses sit 3 grid units higher
  (`anchors(m).eye_lift`); the z's, notes, dots and heart stay at the cat's
  places for every mascot.
- The falk's crest carries the mood the cat's ears carry (`crest_pose`:
  Idle 10°, Happy 2° ×1.08, Alert −4° ×1.22, Sleep 62° ×0.8, Sad 46° ×0.75,
  dancing 6° + 9·sin(beat), Wave 10° + 10·wave). Typing: two feather tips at
  the cat's paw places. Beak opens to sing and on alert.
- The octopus: `arms(expr, pose)`, six arms; Sleep is coiled (curl 1.8),
  dancing arms sway `sin(t/190 ms + i·1.4 + side)`, the right outer arm waves,
  the two left outer arms type. Still arm paths are cached per `Expr`;
  moving arms (dance, wave, a lifted paw) are built per frame while they move.
- Accessories: `cat_style::pieces(mascot, accessory)` with per-mascot
  `anchors`; the cat's seven pieces are byte-for-byte as before (a literal
  copy in the test). New for every mascot: Headphone, Sailor (Topi pelaut),
  Bowtie (Dasi kupu-kupu), appended to `CatAccessory`. See [[cat-accessories]].

## Geometry tweaks the clamp tests forced (recorded per the spec)

- Cat headphone band top 16 → 16.4 and the octopus headphone band pulled in
  0.9 in x: clear of the Music note's dot.
- `NIGHTCAP_TIP_OCTOPUS` 6.5 grid units above the top anchor (the cat and
  the falk keep 14): the pompom left the 160 × 28 notch. The octopus's
  nightcap is shorter on purpose.
- The octopus's Happy outer arms end 0.4 further in: the curl cap touched the
  clock by 0.02 px when looking right.
- The falk needed nothing (Alert crest 1.22, sway ±9, Wave ±10, hop, tips).

## Motion and power ([[bata-idle-power-budget]])

No new timer, id or thread. On the frame clock, only while they last: the
falk's hop on a new Happy notice (`HOP_PX` 1.875 px = 3 grid units in the
40 px badge, 400 ms), the greeting wave on a new notice with the Wave face
(three swings, 900 ms, then the raised pose holds), the octopus's flush on
alert (`sin(π t / 700 ms)` toward #D63A55, the clock runs 700 ms instead of
the bounce's 520). Motion not Full, Energy Saver or Windows animations off:
none of them. The dance and typing ride `TIMER_EQ` and `TIMER_TYPING` as the
cat's do.

"A new notice" is `App::notice_serial`, bumped when the shown `View::Notice`
differs from `notice_shown` by value. Known: two identical notices back to
back give one hop, and coming back to the same notice after the panel gives
a second wave. Accepted (rare).

## Settings ([[settings-structure]])

The Kucing section is **Maskot** / Mascot (`--settings mascot` too): a
first row of three still mascot tiles, the coat swatches of the chosen
mascot (falk swatches show the face colour), the shared accessory tiles,
all ten, in rows of at most five when they do not fit one row. "Maskot di
notch", "Ikuti maskot" for the Auto surface. In the light theme every coat
swatch (the cat's too) has a full `secondary` 1 px edge: the pale coats were
under 3:1 on the light card. Still mouse-only like the other swatches.
The Auto swatch keeps its cat icon (follow-up).

## Stays the cat

Logo, exe icon, tray, notch-peek eyes, landing page and the sticky-note peek
(`widgets/paint.rs::peek_look`).

## Demo

`DI_MASCOT` (`cat`, `falk`, `octopus`), `DI_FALK_COLOR`, `DI_OCTOPUS_COLOR`
follow every scene; `--demo-robot` scenes `maskot-falk`, `maskot-gurita`
(eleven faces), `maskot-warna` (every coat of the three), `maskot-ekstra`
(hop, flush, coiled sleep, both waves). `bata-colors` shows the chosen
mascot's own coats.

## Not verified live

Built and tested only (3,972 tests on `main` after the merge). Not seen on
the real island: each mascot at 28 and 44 dip, solid and see-through, every
coat, the new accessories, the octopus dancing to real music, the falk hop
in a real "Claude selesai", typing while Claude runs (the falk's cheeks do
not follow the typing eye shift), a greeting, a reminder alert, sleep, the
short octopus nightcap, the octopus curl tips on a light wallpaper, and
Settings switching back and forth in both themes.

Related: [[bata-robot-expressions]], [[cat-accessories]],
[[cat-colours-and-surfaces]], [[cat-dances-to-music]],
[[cat-types-while-working]], [[exe-size]]

Listed in [[index]].
