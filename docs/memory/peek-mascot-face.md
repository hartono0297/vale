---
tags: [decision, mascot, notch, peek, painter]
date: 2026-11-15
---

# The shrunk notch shows the chosen mascot's mini face

Owner request (2026-11-15): the "Mengecil jadi mata" eyes were cat eyes for
every mascot. After a mockup of three options (`docs/design/mata-maskot.html`:
A colour only, B colour plus one mark, C "wajah mini") the owner chose **C**:
the pill itself takes the mascot's face colour, the eyes are dark like on the
mascot, plus its small marks. Spec:
`docs/superpowers/specs/2026-11-15-mata-maskot-design.md`.

All three mascots share one face (the same pill eyes), so only colour and
marks can tell them apart at 34 x 14 dip. The falk's crest cannot show: the
pill touches the screen's top edge.

## How it is built

- `src/ui/peek_face.rs`: `peek_face(CatLook, surface) -> PeekFace { fill,
  eye, edge, face_marks, coat_marks }`, pure, one test per coat (11). Cat
  fills come from `CatLook::head_rgb`, eyes from `eye_rgb` (amber for the
  Black cat), the Black cat's edge is its `rim()`; falk from `falk_ink`
  (face, cheeks `EC6B3A`, beak `FALK_BEAK`); octopus from `octopus_rgb` with
  two `shade` spots. Pale coats (White cat, Calico, falk WhiteFace) get a
  1 dip edge in the island surface colour so they do not melt into a white
  title bar.
- Face marks (falk cheeks and beak) slide with the look; coat marks (calico
  patches, octopus spots) stay on the pill. `eyes_offset_in` clamps the face
  marks too: dots are tested as 32 points on the circle, boxes by corners.
  The falk's sideways reach is 3.38 dip instead of 4 (box corners gave 2.29,
  the final review caught it); its downward reach is about 1.44 of 1.5.
- Painting: `RobotPainter::paint_eyes(.., face)` draws eyes, blink lines and
  face marks in one grid transform; `paint_peek_marks` the coat marks.
  `Renderer::paint_eyes_pill` draws coat marks and the edge under the island
  outline mask only (an axis clip there cut the edge on the Notch style's
  shoulders), then the eyes under the shape's axis clip.
- The fill: `Frame::peek_mix` (0 notch surface, 1 face) is eased like the
  see-through `fill` pair (`App::peek_mix`, `PeekMix { from, to }`,
  retargeted from the reached value when `self.eyes` flips at the morph's
  start, settled on the Motion Off / Energy Saver paths). `peek_fill`
  blends the colour with `mascot::mix` and raises the alpha to 1, so a
  see-through notch still shows a solid face. At mix 0 the fill call is the
  old one. `Drawn::peek_mix` is in the dirty-rect signature.

## Power and size

No new timer, id, thread or clock use ([[bata-idle-power-budget]]): the
blend rides the existing 320 ms eyes morph, at rest `peek_mix_at` returns its
target. Exe +4 KiB ([[exe-size]]).

## Verified

`cargo test --locked` green on `main` after the merge. Looked at in the
debug `--demo` (`DI_DEMO_STEP=peek-eyes`, `notch:peek-eyes`,
`peek-eyes-left`, `peek-eyes-right` with `DI_MASCOT` and the coat variables):
cat Terracotta, White, Black, Calico, falk Lutino and WhiteFace, octopus
Coral and Blue, Pill and Notch. Not tried by hand: the real pointer
shrink/restore morph colour blend, a see-through notch, a light title bar
under a pale face, 44 dip notches.

Related: [[notch-peek-eyes]], [[maskot-falk-gurita]],
[[cat-colours-and-surfaces]]. Listed in [[index]].
