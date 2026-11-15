---
tags: [decision, mascot, animation, island, power]
date: 2026-11-15
---

# Mascot stories around the island

Owner request (2026-11-13): the mascot leaves the notch and wraps a tail,
tentacles or wings round it. Eight mockup rounds; the approved reference is
`docs/design/maskot-notch-v3.html` (Notch/Pill switch, `#sheet=<id>:t1,t2`
hook; serve it over http, a `file:` page loses the hash). Spec
`docs/superpowers/specs/2026-11-13-animasi-maskot-notch-design.md`, plan
`docs/superpowers/plans/2026-11-13-animasi-maskot-notch.md` (part 1) and
`docs/superpowers/plans/2026-11-14-animasi-maskot-notch-part-2.md` (part 2).

- **Engine** in `src/ui/play/`: `rim` (a point on the island's edge by arc
  length, both styles), `Deform` (dents, squeeze, swing, shift, tilt; `is_rest`),
  `Beat` (spread, alpha, t), `trigger` (presets, `pool`, `pick`, never the
  last story unless the pool has one), `gate` (permanent / transient /
  idle-moment gates, `flips!` keeps them complete), `stories/` (one module per
  story returning a `Shot`: deform, `DigitFx` per clock character, the mascot
  pose and its parts). Ten stories: cat Syal ekor, Petak umpet, Bantal angka,
  Piano ekor; octopus Penasaran jam, Peluk pulau, Bungee; falk Kelelawar,
  Maling angka, Terbang keliling. Every mascot keeps two stories with the
  clock off.
- **When**: `Config::mascot_play` (Sering, Sesekali default, Jarang, Hanya
  sapaan, Mati; Settings → Maskot → Otomatis). Idle moments need 15-300 s at
  the PC without typing, a cooldown, the plain idle view, no typing face
  (Claude/CLI/mole working), motion Full, no Energy Saver. Also on the hour
  (a clock story) and after a greeting. Right-click → "Mainkan maskot" plays
  one any time the island could show it (it closes an open panel first with
  `Event::PanelShut`, then waits for the morph via `pending: Option<Moment>`).
- **Ends**: only a press on the idle island (`play_press`, from the
  island's own `WM_L/R/MBUTTONDOWN`) or a permanent gate flipping (a panel or
  notice by any path, fullscreen, recording, lock, Energy Saver, the shape or
  mascot changing, the clock jumping). Since 2026-11-15 the pointer, keys,
  the wheel and clicks elsewhere never end it ([[play-ends-on-notch-press]]).
  Always eased home over 200 ms (`WIND_DOWN`), instantly only when motion is
  not wanted or the mascot changes.
- **Layers** (the mockup's `compose`, one `PAINT_ORDER`): Back (clipped to
  window minus the played island, even-odd, before the fill), the fill, Dark
  (clipped to the island), Inside (loose), the clock, Wrap (island ∩ rect),
  Front (loose). A hidden part is cut away, never covered, so a see-through
  fill shows nothing behind. Parts are pure `Draw`s (Fill nonzero, Tint,
  Stroke, Ellipse, Head through the app's own painter, Digit, ClockChar,
  Whistle, Group, Clipped); a fading Group/Head/Clipped goes through one
  opacity layer sized to its ink.
- **Art** in `src/ui/play/art/`: `tube` (the mockup's limb/tube helpers, 64-unit
  grid), `cat` (seated body, legs, paws, tail; Calico patches, Black rim),
  `octopus` (long arms with suckers, the bare mantle via `bare_octopus`),
  `falk` (wings with lagging primaries, body). Colours come from each coat's
  head colour so every coat matches; the head and its accessory are always
  the app's painter.
- **Stories place by the renderer's real layout** (`Stage` + `stage.room()`),
  never a hard-coded one: a clock-off island once put two heads 13 dip apart.
  `Slot { App, Story, Empty }`; `Shot.halo`; `MascotShot.alpha`.
- **No pops** (the contract macro tests every story): rest at both ends; a
  wind-down holds the timeline at `min(t, beat.cut_at)` and paces every fade
  by wind-down TIME (`kept`, ≤ 0.15 per 60 Hz frame). `beat.alpha` follows a
  cubic ease, so a fade keyed on an alpha band is crossed in one frame — the
  gotcha that made every story pop until P23. The slot head and a figure's
  head never add up past one mascot (one complementary crossfade).
- **Carried digits** are placed from the clock character's measured home
  (`Draw::Digit.home`); `DIGIT_X` (one table) only aims, up to 1.6 dip off.
- **Power**: one one-shot `TIMER_MASCOT_PLAY` (19), armed only while a story
  could start; the frame clock only while a play runs; a play forces a Full
  redraw. Nothing runs at idle ([[bata-idle-power-budget]]).
- **Notch shoulders** stay fillets on the screen edge; `top_corners` puts
  their feet where the drawn outline crosses (it follows squeeze, swing,
  stretch, bulge and the island-only `jolt`), so no ledge shows. `Deform`
  also has `stretch` (height: Notch from the edge, Pill about its centre) and
  `bulge_left`; the clock follows only `rigid` (swing, shift, tilt).

Not verified live: none of the ten stories has been looked at on screen
(captures need the owner at an unlocked PC); the right-click path in each
open mode, typing during a play, a mascot or style change mid-play. See [[play-click-through]],
[[maskot-falk-gurita]], [[native-menu-holds-panel]].
