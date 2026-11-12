---
tags: [decision, cat, painter, geometry, v3.8]
date: 2026-11-03
---

# Cat accessories and the paint order (v3.8)

Seven choices (`CatAccessory`: None, Glasses, Bow, Nightcap, Scarf, Flower,
Crown), flat style on the 64 grid, drawn for every face (idle, blink, happy,
sleep, alert, sad, wave, music, sing, think, focus). Data and painters in
`src/ui/cat_style.rs`; `RobotPainter::paint` takes a `CatLook` (the coat and
the accessory) from every caller: the renderer (`Frame::cat`, set by
`app.rs` and the demo), the Settings preview and the accessory tiles. Coats:
[[cat-colours-and-surfaces]].

## The cat is ONE silhouette, painted in this order

1. Black only: the rim under everything: both turned ears stroked 3.2 wider
   than their 5-unit stroke and the head stroked 3.2 wide, #8E7CB8.
2. The silhouette in the base colour: both ears (turned for the droop,
   filled, 5-unit round stroke) and the head, one colour, as before.
3. Calico only: the two patches, as the landing page cat. Orange = (left
   ear EXCLUDE head) UNION (circle (17.217, 28.565) r 13.478 INTERSECT head);
   dark = (right ear EXCLUDE head) UNION (circle (50.696, 52.043) r 10
   INTERSECT head); one geometry per colour, so no anti-aliasing seam shows
   between an ear and its circle. `ear_whole` is the ear exactly as drawn
   (lifted, turned for the droop, fill UNION its `Widen`ed stroke). The site
   paints the cream head over the ears, so the parts of an ear inside the
   head stay cream (assumption: the visible ear above and beside the head is
   fully its colour and follows droop and alert lift). The forehead and the
   face centre stay cream. `silhouette()` is kept for the tests.
4. The face marks.
5. All accessories on top.

The owner rejected: a bow or flower behind the head; a light seam between a
calico ear and the head; ears painted over the head. The test
`calico_patches_match_the_site_in_every_ear_state` checks, for every ear
state (rest, alert lift, asleep, sad): (14, 18) orange and (50, 18) dark (the
ears, not crossed); (12, 30) orange and (50, 50) dark (the circles); the
forehead (32, 30) and the face centre (32, 40) in neither; the ear corners
under the head (38, 27) and (55, 38) in neither; and nothing outside the
silhouette, (3, 30) included (the orange circle is clipped to the head). The
cache holds one entry per ear state. `the_calico_swatch_is_the_sites_conic_gradient`
pins the swatch slices.

Paths are built once: the head, ears and marks as before, the calico
patches per ear state, built from the ears and the head
(`RobotPainter::calico`, keyed by the ear's `up` and
`droop`), and each accessory's pieces (`extras`, behind an `Rc`). Nothing is
rebuilt per frame, no timer is added ([[bata-idle-power-budget]]). A caller
that fades the cat does it with a layer round `paint` (the renderer's
`push_opacity`), so the passes never show through one another.

## The accessories (grid units)

- **Glasses**: two rings of r 6.6 at (24, 40) and (40, 40), stroke 1.9, a
  small bridge; espresso, cream on the Black cat. They follow the eyes
  (the head's slide and tilt, plus the eyes' extra slide).
- **Bow**: at (45, 23) on the right ear, raspberry #D23F6A, knot #F07A9A.
- **Nightcap**: a navy #2F4A7A cone over the dome with its tip to the upper
  left (clear of the z's by the right ear), a cream band and pompom at
  (14.5, 7). The pompom first sat at (10, 5), on the wave's cream heart at
  (4, 1): both cream, they merged into one blob. The test
  `no_accessory_covers_a_faces_cream_extras` keeps every piece's box off
  every face's cream extras (heart, z's, notes, thought dots).
- **Scarf**: a mustard #E0A526 band over the head's base and a short darker
  tail on the left.
- **Flower**: pink #F29BB5, five petals, amber #F2B33D centre, at (16, 23) on
  the left ear.
- **Crown**: a small gold #F2B33D crown on the dome between the ears.

Ear pieces use their ear's transform: the droop turn, and 0.6 of the alert
lift (`EAR_LIFT`; only the tip rises). Every piece follows the look's slide
and tilt (`Pose::looking`). Checked legible at the notch's 26 px and in the
40 px notice badge on the demo sheet `bata-accessories`.

## Clamp test

`the_looking_cat_stays_in_the_idle_island` (`geometry.rs`) now also takes
`accessory_corners(accessory, ears)` (every piece's box, the ear pieces
turned and lifted with their ear): inside the idle island and left of the
clock box, both styles, every notch size 160..300 x 28..44 (each 10 px x 2 px), with and without
the clock, for the five idle faces, open and blinking, at v -1..1. All seven
passed with the shapes above; `LOOK_HEAD_DIP` stays 7
([[cat-looks-at-pointer]]).

Related: [[bata-robot-expressions]], [[settings-structure]]

## Per mascot and three more (2026-11-13)

`pieces(mascot, accessory)` and `accessory_corners(mascot, ..)`: the cat's
seven are unchanged (a literal copy pins them); the falk and the octopus put
the same pieces at their own anchors (`cat_style::anchors`), ear pieces ride
the head there. Three accessories were appended for every mascot: Headphone,
Sailor (Topi pelaut), Bowtie (Dasi kupu-kupu); `CatAccessory::ALL` is ten.
The cat's headphone band top is 16.4 (clear of the Music note). Details and
the octopus's shorter nightcap: [[maskot-falk-gurita]].
