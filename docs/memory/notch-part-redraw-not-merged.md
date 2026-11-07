---
tags: [decision, power, render]
date: 2026-11-07
---

# Part-level redraw of the music views: built, not merged

Round 3 of the 2026-11-06 power work redrew only the notch parts that changed in
the music views (several small dirty rects instead of the whole shape) and
cached text layouts. Spec and plan: `docs/superpowers/specs/2026-11-06-notch-part-redraw-design.md`,
`docs/superpowers/plans/2026-11-06-notch-part-redraw.md`. The code is on branch
`task/notch-parts` (fd853ef): reviewed clean, tests green, the debug self-check
0 mismatches at 100 % scale.

**Not merged**, because the release build measured no clear gain over `main`
(ac5dbbc) with music playing in the lyric view with the dancing cat, 60 s each,
2026-11-07: round 3 at 56 and 70 ms CPU/s (main thread 32 and 44), `main` at 64
(main thread 35), v5.0.0 at 94 (main thread 58). It added ~1,800 lines to 8 files.

Why it saved little there: the dancing cat and a scrolling lyric line change
almost every frame, so the small rects still cover most of the notch. The
compact view, where only the bars move, was not measured on its own; that is
where it could still pay off. Measure that before reviving the branch.

Gotchas the branch found, worth keeping if it comes back:
- An `IDWriteTextLayout` cache keyed by the format's pointer must hold the
  format, or a dropped temporary format's address is reused.
- The art bitmap cache keys covers by length plus the first 64 bytes
  (`art_key`); equal-length JPEGs share their headers, so a song change can
  keep the old cover. The branch added `model::Art` with an id hashed once over
  every byte.

Related: [[bata-idle-power-budget]], [[d2d-dc-target-full-copy]], [[exe-size]]. Index: [[index]].
