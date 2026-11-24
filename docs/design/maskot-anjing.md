# Dog mascot: design draft and hand-over

Status on 2026-11-24: **brainstorming, nothing decided by the owner yet, no app code changed.**
This note lets a new session (on another PC) continue without the old transcript.

## The request

The owner (in Indonesian): "for the mascot I want to add a dog, husky/pom/shiba. Show me the
visuals first as a PDF." The task runs through the Superpowers flow in `AGENTS.md`:
brainstorming → owner's OK on a design doc → `writing-plans` → test-first implementation →
merge into `main` → docs + push → worktree cleanup.

## What exists

- `docs/design/maskot-anjing.pdf`: 8 A4-landscape pages, sent to the owner.
  1. Cover: the three breeds as tiles and on the idle notch (1.5x and real size) next to the
     cat, falk and octopus.
  2. Two pages per breed: 6 expressions (idle, blink, happy, alert, sad, sleep), every coat
     as a tile and on the Plain notch, "Jadi muka" per coat plus happy/blink/sleep, the
     peek pill per coat, and the accessories Dasi kupu-kupu, Headphone, Topi pelaut and a
     new idea, Bandana, plus Jadi muka with Headphone.
  3. Decisions page with two Settings → Maskot sketches (A and B, below).
- `docs/design/maskot-anjing.html`: the source of the PDF, one self-contained file (all
  drawing code inline). To re-print it with headless Edge (PowerShell; a fresh
  `--user-data-dir` is needed or Edge hands off to the running browser and writes nothing):
  `msedge --headless=new --disable-gpu --user-data-dir=<tmp> --no-pdf-header-footer --virtual-time-budget=8000 --print-to-pdf=<out.pdf> file:///<path>/maskot-anjing.html`

### How the HTML is organised (useful when implementing)

- A shared kit (`window.DOGKIT`) on the app's 64-unit mascot grid: the cat's eyes and
  expressions (pills 5.6 x 10 at (24,40)/(40,40)), ear poses (alert raises tips 4, sleep
  droops 12°, sad 24° about the ear pivots), the accessories placed on anchors, the idle
  notch (190 x 34, 26 dip mascot square), Jadi muka (1.28 dip per unit, face fill, marks
  clipped to the notch, clock ink by 4.5:1), and peek (34 x 14 pill, 0.8 dip per unit).
  These numbers come from the code (`src/ui/robot.rs`, `cat_style.rs`, `mascot.rs`,
  `peek_face.rs`, `notch_colour.rs`, `theme.rs`).
- One definition per breed (`window.DOG.husky`, `.pomeranian`, `.shiba`): `coats`
  (label, Auto surface, eye, face, dot, tint, edge/rim), `anchors` (top, tie, hp),
  `head()` (ears + head + coat marks), `over()` (nose, tongue on happy), `faceMarks()`,
  `faceOver()`, `peekMarks()`. The paths and hex values there are the design to port to Rust.

## The three drafts

All three follow the existing rules: one flat silhouette, ears fused behind the head in the
head colour (fill + 5-unit round stroke), the same two pill eyes, no outlines, gradients or
shading. **One deliberate exception: a small dark nose** (about 5 x 3.5 units at (32,47)) and
a tiny pink tongue on Senang. Without the nose the husky reads as a masked cat and the shiba
as a fox.

| Breed | Look | Coats (head colour, Auto surface) | Anchors top / tie / hp |
|---|---|---|---|
| Husky | cat-like but shorter, broader ears; dark cap over head and ears, light mask arching over the eyes with a dark peak between them, two light eyebrow dots | Abu-abu & putih #7E8898 Night (default); Hitam & putih #2C2A32 Plum + rim #8E7CB8; Merah tembaga & putih #A9643E Espresso; Putih salju #D6CFC6 Sea. Face #F6F1EA, eyes #17120F, surface edge on all | [32,22.5] / [32,54.5] / [7,57,41,14.5] |
| Pomeranian | round ball of fluff (12 bumps, widest at the cheeks), tiny ears peeking out on top, lighter oval muzzle | Oranye #EE9B3A Espresso (default); Krem #F2DDB8 Night + surface edge; Coklat #B07A4F Graphite; Hitam #1E1B22 Plum, amber eyes, rim #8E7CB8, tan muzzle | [32,18.5] / [32,57] / [6,58,40,13] |
| Shiba | wider head with full low cheeks, short thick ears; urajiro: cream cheeks and muzzle plus two cream eyebrow dots | Merah #D88A35 Night (default); Wijen #B87A42 Sea (dark sesame on ears and forehead); Hitam & tan #27211F Plum, amber eyes, rim, tan eyebrows; Krem #E6CB9C Graphite + surface edge | [32,23] / [32,53.5] / [6,58,42,15.5] |

## Open questions (owner's to decide)

1. **Breeds:** one, two, or all three. (The first question asked was: which one do you like
   most, and why? No answer yet.)
2. **Settings → Maskot:** (A) one "Anjing" tile with a Ras row under it (the mascot row stays
   four tiles), or (B) each breed is its own mascot tile (the row grows to 4-6).
3. **Coats per breed:** keep all four or trim. Agents flagged: husky Putih salju and pom/shiba
   Krem are faint on Jadi muka and peek; shiba Wijen uses a third colour (like Calico);
   husky Hitam & putih needs the purple rim to show on Plum; a white pom was left out
   because it looks like Krem and the white cat.
4. **Nose:** keep (recommended) or drop.
5. **Mascot stories** (`src/ui/play/stories/`): own stories like cat/falk/octopus, or face
   on the notch only for now.
6. **Accessories:** the ten existing ones (all already placed through anchors), plus the new
   Bandana or not.
7. Smaller: shiba cream inner ears (left out because the cat has no inner ear); with the bow
   tie on, the happy tongue is partly hidden.

## Next steps

1. Get the owner's answers to the open questions (one at a time), then lay out the build
   approaches (e.g. one `Mascot::Dog` with a breed field vs one variant per breed).
2. Write the design doc in `docs/superpowers/specs/2026-11-24-maskot-anjing-design.md` and
   get the owner's OK.
3. `superpowers:writing-plans`, then implement test-first. What a new mascot needs is
   listed in `docs/memory/maskot-falk-gurita.md` (and the clamp tests that iterate
   `Mascot::ALL`).
