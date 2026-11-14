---
tags: [decision, mascot, animation, island, power]
date: 2026-11-14
---

# Mascot stories around the island (part 1)

Owner request (2026-11-13): the mascot leaves the notch and wraps a tail,
tentacles or wings round it. Eight mockup rounds; the approved reference is
`docs/design/maskot-notch-v3.html` (Notch/Pill switch, `#sheet=<id>:t1,t2`
hook; serve it over http, a `file:` page loses the hash). Spec
`docs/superpowers/specs/2026-11-13-animasi-maskot-notch-design.md`, plan
`docs/superpowers/plans/2026-11-13-animasi-maskot-notch.md`.

- **Engine** in `src/ui/play/`: `rim` (a point on the island's edge by arc
  length, both styles), `Deform` (dents, squeeze, swing, shift, tilt; `is_rest`),
  `Beat` (spread, alpha, t), `trigger` (presets, `pool`, `pick`, never the
  last story unless the pool has one), `gate` (permanent / transient /
  idle-moment gates, `flips!` keeps them complete), `stories/` (one module per
  story returning a `Shot`: deform, `DigitFx` per clock character, the mascot
  pose, props). Everything a story draws is clipped to the played island.
- **When**: `Config::mascot_play` (Sering, Sesekali default, Jarang, Hanya
  sapaan, Mati; Settings → Maskot → Otomatis). Idle moments need 15-300 s at
  the PC without typing, a cooldown, the plain idle view, no typing face
  (Claude/CLI/mole working), motion Full, no Energy Saver. Also on the hour
  (a clock story) and after a greeting. Right-click → "Mainkan maskot" plays
  one any time the island could show it (it closes an open panel first with
  `Event::PanelShut`, then waits for the morph via `pending: Option<Moment>`).
- **Ends**: pointer near the island, any mouse button, a key or a hotkey
  (mouse movement alone does not), a gate flipping; always eased home over
  200 ms (`WIND_DOWN`), instantly only when motion is not wanted or the
  mascot changes.
- **Ready filter**: only stories with a `Shot` are in the pool
  (`stories::ready`, `trigger::has_ready`). Part 1 ships the falk's "Maling
  angka" (needs the clock); the cat and the octopus play nothing yet and the
  Settings row is dimmed for them with "Animasi untuk maskot ini menyusul."
  Part 2 adds the other nine stories, one module each, and flips `ready`.
- **Power**: one one-shot `TIMER_MASCOT_PLAY` (19), armed only while a story
  could start; the frame clock only while a play runs; a play forces a Full
  redraw. Nothing runs at idle ([[bata-idle-power-budget]]).
- **Notch shoulders** stay static fillets on the screen edge (they follow the
  squeeze, ignore the swing, tuck 3 dip into the body); a story that dents a
  top corner more than 3 dip would show a ledge.

Not verified live: the right-click path in each open mode, typing during a
play, a mascot or style change mid-play. See [[play-click-through]],
[[maskot-falk-gurita]], [[native-menu-holds-panel]].
