---
date: 2026-11-15
tags: [design, mascot, notch, peek]
---

# The shrunk notch shows the chosen mascot's face

## Intent

Owner request (2026-11-15): "ubah shrink to eye, biar sesuai mata maskot, jadi
jangan kucing semua". With "Mengecil jadi mata" the notch shrinks to a 34 x 14
dip pill with two terracotta eye pills, whatever mascot is chosen (Settings →
Maskot: cat, falk, octopus). The falk and the octopus should be recognisable
there too.

All three mascots share one face (the same pill eyes), so the mascot can only
show through colour and a few small marks. Mockup with the options and every
coat: `docs/design/mata-maskot.html`. The owner picked **C, "wajah mini"**:
the pill itself takes the mascot's face colour, the eyes are dark like on the
mascot, plus that mascot's small marks.

## The face per coat (owner approved)

Colours are the app's own constants (`mascot.rs`, `cat_style.rs`).

| Mascot / coat | Pill fill | Eyes and blink | Marks | Edge |
|---|---|---|---|---|
| Cat Terracotta | `E2683C` | espresso `17120F` | none | none |
| Cat Calico | cream `F4EDE4` | espresso | orange patch top-left `E2683C`, dark patch bottom-right `2B2420` (coat, fixed) | surface |
| Cat White | `F6F1EA` | espresso | none | surface |
| Cat Black | `1E1B22` | amber `F2B33D` | none | rim `8E7CB8` |
| Falk Lutino / Grey | face `F6D04A` / `F2D35A` | espresso | cheeks `EC6B3A`, beak `D9978F` (face, slide) | none |
| Falk WhiteFace | face `F4EFE6` | espresso | beak only (no cheeks, as on the falk) | surface |
| Octopus (four coats) | coat colour | espresso | two spots, the coat 18% darker (coat, fixed) | none |

"Surface" is the island's surface colour now (`palette.surface`), a 1 dip line
on the pill's sides and bottom so the pale faces do not melt into a white
title bar. Contrast of the eyes on every fill was checked (lowest: espresso
on octopus Red, 4.66:1).

Mark geometry, in the eyes' grid (the 64-unit grid `paint_eyes` draws in, eyes
at (24, 40) and (40, 40), `EYES_SCALE` 0.8 dip per unit), taken from the
mockup:

- Falk cheeks: dots at (18.75, 44) and (45.25, 44), r 2.375.
- Falk beak: `M30.375 43 Q32 42 33.625 43 Q33.5 45.5 32 46.375 Q30.5 45.5
  30.375 43 Z`.
- Octopus spots: (44.25, 36.5) r 1.625 and (19, 35.75) r 1.125.
- Calico patches: (17.625, 35) r 4.25 and (47.625, 45.625) r 3.75, clipped to
  the pill.

## Behaviour

- **Face marks** (eyes, cheeks, beak) slide together with the look.
  `eyes_offset_in`'s clamp covers all of them, so the falk's sideways look is a
  little shorter (cheeks stay whole). **Coat marks** (calico patches, octopus
  spots) are fixed to the pill and clipped to it.
- **Blink**: the same two level lines, in the eye colour. Timing unchanged.
- **The morph (320 ms, `EyesInOut`)**: the fill colour blends from the
  surface to the face colour along the same eased progress, and back on a
  restore. Motion Saver / Off, Energy Saver: at once.
- **See-through notch**: the pill is solid face colour (its alpha blends to 1
  with the same progress); it is a face, not a background.
- **"Maskot di notch" off**: still the chosen mascot's face.
- **Changing mascot or coat**: the next frame shows it.
- **Unchanged**: pill size and place, hit test and click-through, when it
  shrinks or restores, the look and blink timers, the poll. No new timer,
  thread or id ([[bata-idle-power-budget]]). Exe growth a few KB at most.

## Approaches

1. **Chosen: a small face table plus the existing eyes painter.** A pure
   `peek_face(CatLook) -> PeekFace` (fill, eye, edge, face marks, coat marks),
   drawn by the existing `paint_eyes` / `paint_eyes_pill` path; the clamp
   takes the face marks' boxes. Cheap, testable, keeps blink and look.
2. Rejected: the real mascot painter scaled into the pill. At 14 dip the
   ears and crest are cut into noise, and every frame costs more.
3. Rejected: pictures per coat. The exe grows, blink and look stop being
   smooth.

## Left to the builder

- Where `PeekFace` lives (`robot.rs` next to the eyes, or `mascot.rs`) and how
  the morph's face mix reaches the frame (likely an App pair like `fill`,
  eased like `fill_at`).
- How the edge is stroked in the Notch style (its shoulders): the line must
  show on the sides and bottom; along the top screen edge is acceptable.
- Demo scenes: the `peek-eyes*` scenes follow `DI_MASCOT` and the coat
  variables; one scene per mascot is enough.
- Updating `docs/memory/notch-peek-eyes.md` (it says the pill stays
  terracotta) and `maskot-falk-gurita.md` ("Stays the cat" lists the peek
  eyes).
