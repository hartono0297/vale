---
tags: [spec, mascot, cat, painter, settings]
date: 2026-11-12
---

# Two new notch mascots: the falk (cockatiel) and the sitting octopus

## Intent

Vale's notch has one mascot, the cat. The owner wants a choice of mascot: a
**falk** (cockatiel, from a photo of the owner's own bird) and a **sitting
octopus** ("gurita duduk"). Both must feel like the same family as the cat:
the 64-unit grid, flat colours, espresso pill eyes, one silhouette, the same
eleven faces, the same accessories, and the same motion rules. The idle cost
stays zero ([[bata-idle-power-budget]]).

The mockups the owner chose from are in `docs/design/`:
`maskot-falk-gurita-animasi.html` (falk A crest, octopus, accessories,
motion), `maskot-falk-sayap.html` (falk without wings, "Pilihan 1" chosen) and
`maskot-pilihan-akhir.html` (the last choices). Their JS painters hold every
coordinate below and can be read as the reference drawing.

## Goals

1. Settings → **Kucing** becomes **Maskot**: choose Kucing, Falk or Gurita.
   The cat stays the default, so nothing changes for anyone who does not pick.
2. The falk and the octopus are drawn by the same painter path as the cat
   (`RobotPainter::paint`, `CatLook`) everywhere the notch mascot shows: the
   compact views, the island's own notices (40 px badge), the Settings preview
   and chips.
3. Every face (`Expr::ALL`, 11) has a version for each new mascot; the
   pointer look, blink, hover lift, alert bounce, dance, typing and sleep
   behave as for the cat and run on the same timers.
4. Three new accessories for all three mascots: **Headphone**, **Topi
   pelaut** (sailor cap), **Dasi kupu-kupu** (bow tie).
5. Coats: the falk has three, the octopus four; the island surface `Auto`
   follows them.
6. Three extra motions the owner chose (below): octopus flush on alert, falk
   hop on a "done" notice, octopus arms coiled while asleep.

## Non-goals

- The landing page (`site/`), the logo, exe icon, tray icon, README images
  and the notch-peek eyes stay the cat ([[logo]], [[notch-peek-eyes]]).
- No wings and no tail on the falk (the owner rejected both).
- No new repeating timer, no idle frame clock, no new thread.
- No rename of `robot` / `bata` / `cat_*` in code beyond what the feature
  needs (the project's "no rename churn" rule, [[bata-robot-expressions]]).

## Anti-goals (a failure even if it works)

- The falk reading as a chick, or the octopus as a jellyfish: the owner
  rejected both in earlier rounds. The falk's tall wispy crest, orange cheeks
  and pink beak, and the octopus's balloon mantle with low eyes and sideways
  curled arms with suckers are what carry the species.
- Anything moving at idle (crest sway or arm sway without a trigger).
- A piece leaving the island at any notch size (the clamp tests).

## The falk ("Falk A, pilihan 1")

Head only, like the cat (which is a head and ears). Grid units, 64 grid.

- **Body**: an egg, `M9 43Q9 22 32 22Q55 22 55 43Q55 57 32 57Q9 57 9 43Z`.
- **Face mask**: ellipse (32, 33) rx 22 ry 17 in the face colour, clipped to
  the body (so the lower body shows the body colour).
- **Crest** (the cat's ears' job): a base ellipse (32, 26) rx 5.5 ry 4 and
  four tapered feathers, each a quad leaf from base to tip with a half-width
  and a sideways bend (base x, base y, tip x, tip y, half-width, bend):
  (28, 27, 27, 11, 2.8, -3.5), (30.5, 26, 35, 3, 3.2, -5),
  (33, 26, 42, 6, 3.2, -5), (35.5, 27, 46, 14, 2.6, -3.5). The leaf: normal
  n of base→tip, mid = midpoint + n·bend, path `M base Q (mid + n·w) tip Q
  (mid − n·w) base Z`. The crest group is rotated round (32, 27) and scaled
  round the same point per face (table below). Painted behind the body.
- **Cheeks**: circles (15.5, 44.5) and (48.5, 44.5), r 4.4 (none on Muka
  putih). They follow the face's slide at half the eyes' slide (like the
  mockup's `under` layer) so they stay on the head.
- **Beak**: `M29.4 44.2Q32 42.4 34.6 44.2Q34.4 48.4 32 49.8Q29.6 48.4 29.4 44.2Z`
  #D9978F. Open (Sing, Music while dancing, Alert): upper
  `M29.4 43.8Q32 42 34.6 43.8Q34.2 46.8 32 47.6Q29.8 46.8 29.4 43.8Z`, lower
  `M30.4 49.2Q32 48.6 33.6 49.2Q33 51.2 32 51.4Q31 51.2 30.4 49.2Z`. The beak
  is a face mark: it follows the eyes' slide and glance.
- **Eyes**: the cat's marks at the cat's places, unchanged.
- **Typing**: no paws; two feather tips on the keys at the cat's paw places
  (1, 52.6) and (9.5, 52.6), lifted by `paw_lift`, in the body colour:
  `M(x−4) (y+1) Q(x−3.5) (y−2.6) x (y−2.6) Q(x+4.4) (y−2.4) (x+4) (y+1.2)
  Q x (y+2.4) (x−4) (y+1) Z`.

Crest per face (rotation in degrees clockwise round (32, 27), scale):

| Face | Crest | Notes |
|---|---|---|
| Idle, Focus, Think, Typing | 10°, 1.0 | relaxed, leaning back |
| Happy | 2°, 1.08 | a little more upright |
| Music, Sing (dancing) | 6° + sway ±9° with the beat | sway = `sin(π t / 600 ms)·9`, the same clock as the nod; at rest (not dancing) 6° |
| Alert | −4°, 1.22 | stands up, the cockatiel's real alarm |
| Sleep | 62°, 0.8 | flat back |
| Sad | 46°, 0.75 | down |
| Wave | 10°, waving: ±10° three times, then still | see "Greeting" |

Coats (`FalkColor`): body / face / crest / cheek.

| Coat | Body | Face | Crest | Cheek | Auto surface |
|---|---|---|---|---|---|
| Lutino (default) | #F1E4A2 | #F6D04A | #F6D04A | #EC6B3A | Espresso |
| Abu-abu | #9A959C | #F2D35A | #E9CF62 | #EC6B3A | Night |
| Muka putih | #9A959C | #F4EFE6 | #E4DED6 | none | Plum |

Eyes are espresso #17120F on every falk coat (they sit on the face mask).

## The sitting octopus ("gurita duduk")

- **Mantle**: `M32 9C50 9 55 25 50 37Q47 45 40 46H24Q17 45 14 37C9 25 14 9 32 9Z`.
- **Spots**: shade (coat × 0.82 toward black) circles (22, 20) r 2.2,
  (40, 15) r 1.6, (45, 24) r 1.3.
- **Eyes**: the cat's marks shifted up 3 grid units (eye centres at y 37),
  through the face's eye offset (the same mechanism as Typing's
  `eye_shift`, added to it).
- **Six arms**, behind the mantle, stroke 5, round caps and joins, in the coat
  colour. Each is a quad from a start through a control to an end, then a
  curl: `q (o·3.4k) 0.2 (o·3.2k) (−3.2k) q −0.2 (−2.4k) (−o·2.4k) (−2.2k)`
  where o is the curl's side and k the curl amount (1 at rest). Left side
  (start, control, end): (19, 42)→(8, 51)→(3, 54); (24, 45)→(15, 57)→(9.5, 57.5);
  (29, 46)→(27, 58)→(21, 58.5). The right side mirrors x (64 − x). The outer
  two pairs curl outward, the inner pair toward the middle.
- **Suckers**: two dots r 0.85 per arm at t 0.55 and 0.8 along the quad,
  1.6 below it, in the coat mixed 55 % toward white.
- **Typing**: the two outer left arms reach the keys: control (start x − 8,
  50), end (1, 53 − lift) and (9.5, 53 − lift), curl 0.3, lifted by the
  paws' rhythm (`TypingPaws`).

Arms per face:

| Face | Arms |
|---|---|
| Idle, Focus, Think, Typing (right side) | rest |
| Happy | ends up 3, curl 1.25 |
| Music, Sing (dancing) | each end sways `sin(t/190 ms + i·1.4 + side)`: x ±2, up to 2.5 up; on the same clock as the nod |
| Alert | splay: ends 2.5 outward and 2.5 up, curl 1.3; plus the flush (below) |
| Sleep | **coiled** (chosen extra): curl 1.8, each end 4 grid units toward the middle, 1 up |
| Sad | limp: curl 0.4, ends 1 down |
| Wave | the right outer arm raised: control (58, 40), end (60, 28), waving (see "Greeting") |

Coats (`OctopusColor`):

| Coat | Colour | Auto surface |
|---|---|---|
| Koral (default) | #E87C8E | Espresso |
| Merah | #D9505F | Graphite |
| Ungu | #A98BD6 | Night |
| Biru laut | #5FA8C9 | Sea |

Measured on their surfaces (non-text, at least 3:1 wanted): Merah on
Graphite 4.25:1, Ungu on Night 5.94:1, Biru laut on Sea 6.15:1, Koral on
Espresso 6.82:1.

## Extras the owner chose

- **Octopus flush on Alert**: the body (mantle, arms, not suckers or spots)
  goes from the coat toward #D63A55 and back, `sin(π t / 700 ms)`, from the
  alert's start. It rides the alert bounce's frame clock, which runs 700 ms
  instead of 520 ms for the octopus.
- **Falk hop on a "done" notice**: a notice that shows the Happy face
  (Claude done, a finished clean, pomodoro done, "Disalin") gives the falk one
  hop of 3 grid units up, `sin(π t / 400 ms)`, at the notice's start, on the
  frame clock, then still.
- **Octopus coiled while asleep**: the Sleep row above.

## Greeting (Wave)

The cat's Wave face is still. For the new mascots the wave moves once: the
falk's crest or the octopus's raised arm swings three times (±10° for the
crest, ±1.5 x / ±3 y for the arm end, period about 300 ms, about 900 ms in
all) when the greeting notice appears, on the frame clock, then holds the
raised pose. With Motion not Full, Energy Saver or Windows animations off it
does not move. This is the only motion the cat does not have; it is a
one-shot like the alert bounce (owner agreed in the playback).

## Accessories

`CatAccessory` keeps its seven values and appends **Headphone**, **Sailor**
and **Bowtie** (appended so stored indices and names keep their meaning).
They are drawn for every mascot and every face, after the face marks, like
the others ([[cat-accessories]]). Each mascot has anchors (grid units):

| Anchor | Cat | Falk | Octopus |
|---|---|---|---|
| top (crown, caps) | (32, 24) | (32, 26) | (32, 9) |
| left (flower) | (16, 23) | (20, 29) | (19, 17) |
| right (bow) | (45, 23) | (44, 29) | (45, 17) |
| neck band (scarf y0..y1) | 49.5..54 | 48.5..53.5, clipped to the body | 43.5..47.5, a rounded band x 14..50 |
| bow tie centre | (32, 51.5) | (32, 52.5) | (32, 45.5) |
| headphone x0, x1, cup y, band top | 7, 57, 41, 16 | 9, 55, 40, 17 | 11, 53, 33, 3 |

The cat's existing accessories keep their exact geometry; the anchors above
are where the new mascots put the same pieces. Shapes (from the mockup):

- **Headphone**: cream #F4EDE4 band, stroke 2.6, `M(x0+1) (y−3) C x0 top x1
  top (x1−1) (y−3)`; cream cups 7 × 12 r 3 at (x0−3, y−6) and (x1−4, y−6) with
  a #2F4A7A inner 3.8 × 7 r 1.6.
- **Topi pelaut**: #F6F1EA crown `M(tx−11) (ty+1.5) Q(tx−12) (ty−8) tx (ty−9)
  Q(tx+12) (ty−8) (tx+11) (ty+1.5) Z`, #2F4A7A band (tx−11.2, ty−2.4, 22.4 ×
  3.6) and brim (tx−13, ty+0.6, 26 × 2.4 r 1.2).
- **Dasi kupu-kupu**: two #2F4A7A triangles from the centre (±6, ±3.6), stroke
  1.2 round join, knot r 1.6 #5878AE.

Per-mascot adjustments: the falk's crest stays **behind** caps and the crown
(it pokes out above them); the falk's crown gets a 0.9 #7A5210 outline (gold
on a yellow head); the falk's scarf is #2F4A7A (mustard disappears on
Lutino). The Black cat's rim and amber eyes are unchanged and do not apply to
the new mascots.

## Settings and config

- Config (all lenient, island-owned, in `merge_owned`, like `cat_color`):
  `mascot` (`Cat` | `Falk` | `Octopus`, default `Cat`), `falk_color`
  (`Lutino` | `Grey` | `WhiteFace`, default `Lutino`), `octopus_color`
  (`Coral` | `Red` | `Purple` | `Blue`, default `Coral`). `cat_color` and
  `cat_accessory` keep their keys; the accessory is shared by all mascots.
  Each mascot keeps its own coat, so switching back restores it. Old files
  load unchanged; a typo falls back per field.
- `CatLook` carries the mascot and its coat; every caller that builds one
  (renderer, Settings preview and chips, the demo) passes the config's
  values. `Surface::resolve` resolves `Auto` from the look (table above;
  `Auto` never resolves to Terminal, robot off still gives Espresso).
- Settings → the "Kucing" section is renamed **Maskot** / **Mascot**. At its
  top a three-tile choice Kucing / Falk / Gurita, each tile a small still
  preview of that mascot. Below it the coat chips show the chosen mascot's
  coats (falk 3, octopus 4, cat 4 as today) and the accessory tiles and the
  preview draw the chosen mascot. "Kucing di notch" becomes "Maskot di notch"
  / "Mascot in the notch", its description "Tampilkan maskot Vale di samping
  jam dan notice" / the English equivalent; other labels that say "kucing"
  about the notch mascot say "maskot" (the builder lists them). The preview
  stays a still picture.

## Motion and power

Everything runs on what exists: `TIMER_BLINK` (blink), `TIMER_POINTER` and
the frame clock (look, glance), `TIMER_EQ` (dance: the falk's crest sway and
the octopus's arm sway use the dance's own elapsed time), `TIMER_TYPING`
(the falk's feather tips and the octopus's typing arms), the frame clock for
the hover lift, the alert bounce (700 ms for the octopus with the flush), the
falk's hop (400 ms) and the greeting wave (about 900 ms). The gates are the
cat's (`dance_on`, `typing_anim_on`, motion policy, Energy Saver, Windows
animations, fullscreen, screen away). Nothing else moves at idle.
`timer_ids_never_collide` needs no new id.

## Geometry

- The existing clamp tests (`the_looking_cat_stays_in_the_idle_island`,
  `the_dancing_cat_stays_in_every_compact_shape`,
  `the_typing_cat_stays_in_the_idle_island`,
  `robot_and_content_share_every_compact_shape`,
  `no_accessory_covers_a_faces_cream_extras`) run for all three mascots: every
  piece (crest at its tallest, Alert's 1.22 scale included; arms with their
  curls and suckers; feather tips; every accessory, the new three included)
  inside the island at every notch size and style. Where a piece does not fit
  (the falk's crest tip at y about 3 is the likely one at 28 dip), the builder
  shrinks that piece (a smaller crest scale or shorter feathers) rather than
  changing the island; any such change is recorded in the memory note.
- The see-through legibility halo (`render.rs::paint_robot`) covers each
  mascot's own silhouette box (the falk's body and crest, the octopus's mantle
  and arms) instead of the cat's head rect.
- The notice badge's 40 px cat square holds the new mascots the same way.

## Demo

`--demo-robot` and every `--demo` scene follow a `DI_MASCOT` env var (`cat`,
`falk`, `octopus`) and the coat env vars, like `DI_CAT_COLOR`. The face sheet
and `bata-accessories` draw the chosen mascot; new scenes show each new
mascot's eleven faces, its coats and the three extras at a fixed pose.
Debug only, no real data.

## Settled after the builder check (spec author's calls, from the mockups)

- **The seven existing accessories on the new mascots** are re-placed, not
  scaled: each piece is drawn relative to that mascot's anchor exactly as the
  mockup's `accessory()` does (`docs/design/maskot-falk-gurita-animasi.html`):
  Bow at the right anchor (two triangles ±6 × ±4 and a knot r 1.9), Flower at
  the left anchor (five petals r 2.5 at distance 3, centre r 1.8), Crown on
  the top anchor (base at top y + 1, 7 wide each side, 8 up), Nightcap as a
  cone from (tx − 15, ty + 3) and (tx + 14, ty + 2) to the tip (tx − 13,
  ty − 14) with the cream band and pompom, Scarf as the band of the anchor
  table with a 4.5-wide tail hanging from x 13 (cat, falk) or 17 (octopus),
  Glasses on the eyes (they follow the eye offset, so up 3 on the octopus).
  On the octopus the nightcap tip lands at about y −5: allowed if the clamp
  tests pass, otherwise the cone is made shorter for the octopus only. The
  cat's own pieces keep today's geometry exactly.
- **Accessories that ride the cat's ears** (`Anchor::Ear`: bow, flower) ride
  the **head** on the falk and the octopus: the head's slide, tilt, bob, nod
  and squash, nothing from the crest or the arms.
- **The whole-body transforms apply to all three**: each face's `bob`
  (Alert −1.5, Sleep and Sad +1.5) and `tilt` (Wave −7°, Typing −4°), the
  pointer look's slide and tilt, the dance's nod and squash round (32, 54),
  the rotation round (32, 40). The octopus's arms and the falk's crest are in
  that same group, so they squash and tilt with the body; the clamp tests see
  the transformed pieces.
- **The face extras** (the notes at (57, 13) and (64, 3), the Think dots, the
  z's, the Wave heart at (4, 1)) stay at the cat's grid places for every
  mascot. If a clamp or the extras test fails for a mascot, that mascot's
  piece moves, not the extra.
- **Typing**: the laptop is unchanged and drawn for every mascot; the −4° tilt
  and the (−3.5, 1) eye shift apply to all (on the octopus added to its −3
  eye lift).
- **The hop and the greeting wave start once per notice**: the falk's hop
  fires when an island notice (`Spot::Notice`) whose face is Happy appears,
  every Happy sort, not hover and not the idle CLI Done face; the wave fires
  when a greeting notice appears. Each new notice restarts it, even if the
  face before was the same (a start instant per notice, recorded like the
  alert's, not a face-change edge). Hover lift and the hop never meet
  (hover does not hop).
- **The octopus's alert**: the bounce stays the cat's (2 × 260 ms); the flush
  is one `sin(π t / 700 ms)` pulse from the alert's start, spanning both hops
  and 180 ms past them; the frame clock runs to 700 ms for it. The existing
  bounce tests keep their numbers.

## What is left to the builder

- Type names (`Mascot`, `FalkColor`, `OctopusColor`), where the per-mascot
  painters live (a new module next to `cat_style.rs` is fine), how the face
  table maps to per-mascot poses, path caching (built once, keyed like the
  calico patches; nothing rebuilt per frame).
- Exact i18n English strings.
- Small coordinate tweaks for legibility at 26 px or for the clamp tests,
  recorded in the memory note.

## Done means

Built test-first; `cargo test --locked` green on `main` after the merge; the
exe size change recorded in [[exe-size]]; memory notes (a new mascot note
linked from the index, the updates to [[cat-accessories]],
[[cat-colours-and-surfaces]], [[settings-structure]],
[[bata-idle-power-budget]]) and `README.md` updated; `main` pushed; worktree
cleaned; a hand-check list reported (each mascot in the real notch at 28 and
44 dip, see-through, every coat, the new accessories, dance with real music,
typing with Claude running, greeting, alert, sleep, Settings switching).
