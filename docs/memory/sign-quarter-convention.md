---
tags: [gotcha, sign, geometry]
date: 2026-11-21
---

# Sign: the two quarter directions

`Quarter` counts 90 degree steps, but its direction depends on what it describes:

- **Page rotation** (`PageGeom.base`, `Page.extra`): clockwise as displayed, like PDF `/Rotate`. `turn_cw` is +1 step, so `R270.turn_cw() == R0`.
- **Item turn**: counter-clockwise in page space, like the PDF `cm` matrix (y up).
- So `upright_turn(base + extra)` returns the **same** quarter value (page `/Rotate 90` gives item turn R90), not the inverse number.

Items are stored in the page's unrotated space; rotating a page turns its items with the content. The plan's first Task 2 test expected R270 and was wrong (it mixed the two directions); the plan was amended. If this is ever wrong, signatures appear rotated 180 degrees on rotated pages. `initials_on_all` recomputes the turn with `upright_turn` per target page.

Part of [[tanda-tangan]]; the writer side is [[sign-pdf-writer]].
