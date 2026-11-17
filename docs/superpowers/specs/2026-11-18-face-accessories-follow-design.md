# Jadi muka: accessories move with the face

Date: 2026-11-18. Owner, after trying the Warna notch build: "ketika mode face,
accesorisnya dibikin ngikut gerak juga dong, kalau engga aneh jadinya".
Approved: "Setuju, kerjain". Follows
`docs/superpowers/specs/2026-11-17-warna-notch-design.md` and
`docs/memory/notch-colour-style.md`.

## Intent

In Jadi muka the notch is the mascot's face. The face marks slide with the look
at the pointer and nod on the beat, but of the relocated accessories only the
glasses follow the look; the hat brims, bow tie, headphone earcups, scarf, bow
and flower stay put, so they come apart from the face. The face and everything it
wears must move as one, like a head.

## Design

- **Look:** every accessory piece shown in Jadi muka (glasses, hat brims with
  their tip/pompom/tails, bow tie, earcups, scarf band and tail, bow, flower)
  slides by the same offset as the eyes.
- **Nod (music views):** every piece moves with the face's nod.
- **One clamp for the group:** the slide allowed is the largest one for which the
  face marks AND every worn piece stay inside the island shape (both styles,
  every notch size 160..300 x 28..44, every Face view) and left of the clock /
  content. If a piece would leave the shape, the whole group's slide is cut, so
  the face never moves further than its accessories.
- **Blink:** eyes only, accessories never blink.
- **Shrink to the eyes pill:** unchanged (accessories fade out, as now).
- **Not changed:** Plain and Gradasi (the head already carries its accessories),
  what each view wears, colours and edges (the 3:1 edge rule is judged at rest,
  as now), no new timer or clock.

## Approaches

- Chosen: one shared offset for the face and its pieces, one clamp over all of
  them (a head moves as one piece).
- Rejected: each piece follows with its own clamp (pieces would drift apart at the
  edges, which is the oddness the owner pointed out); only some pieces follow
  (the earcups and scarf would still lag).

## Done means

Merged into `main`, `cargo test --locked` green, a clamp test proving face + all
worn pieces stay inside for every view, size, style, expression, accessory and
look/nod extreme, a test that every piece's offset equals the eyes' offset,
memory note updated, `main` pushed, installed on the owner's PC for a hand check.
