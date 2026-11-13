---
tags: [spec, mascot, animation, notch, timers, power, settings]
date: 2026-11-13
---

# The mascot plays with the island: ten little animations

## Intent

Today the notch mascot reacts, but it never acts. It blinks, it looks at the
pointer, it hops on a notice, it dances to music. It never leaves its slot and
it never touches the notch itself.

The owner asked for animations **around** the notch: the mascot transitions out
of its slot and its tail, tentacles or wings wrap around the notch, and the
notch is a physical object the mascot plays with. The mockups went through
seven rounds; `docs/design/maskot-notch-v3.html` holds the ten that were
accepted, with the full JS painters, a character sheet, a storyboard per story,
a 1:1 strip and per-story Direct2D notes. **That file is the reference
drawing**: every coordinate, timing curve and deformation below can be read
from it.

Two earlier mockup rounds, `docs/design/maskot-animasi-notch.html` and
`docs/design/maskot-animasi-notch-melingkar.html`, were rejected whole. They
are kept only as a record of what not to build (see Anti-goals).

## Goals

1. Ten animations, three to four per mascot, drawn in the island's existing
   layered window around the closed island, as in the mockup.
2. **Both styles.** Every story plays on the **Notch** and on the **Pill**, at
   every size the sliders allow (160-300 wide, 28-44 tall), and with a solid
   or a see-through fill.
3. The island itself reacts: it swings, dents, bulges, shifts, trembles, and
   the clock digits are pushed, lifted, knocked over and swapped.
4. They play at moments that feel chosen, not random: when the owner is at the
   PC but not typing, at the top of the hour, with the greeting.
5. **Right-click on the island plays one now**, the owner's explicit request.
6. The idle cost stays zero ([[bata-idle-power-budget]]): no repeating timer,
   no thread, one one-shot timer id, the frame clock only while an animation
   runs (about eight seconds).
7. Nothing ever gets in the way: an animation yields to any real use of the
   island, and it unwinds instead of being cut.

## Non-goals

- The landing page (`site/`), the logo, the exe and tray icons, the
  notch-peek eyes and the sticky-note cat peek stay as they are
  ([[logo]], [[notch-peek-eyes]], [[widgets]]).
- No animation on the **open panel** or on the island shrunk to the eyes. The
  closed island only, in both of its styles.
- No new repeating timer, no new thread, no polling, no new `WM_APP`.
- No new mascot, coat or accessory. The mascots, faces and coats of
  [[maskot-falk-gurita]] and [[cat-accessories]] are unchanged; this feature
  only adds bodies and limbs that were never drawn before.
- No sound. The "Hatchi", the chirps and the plop in the mockup are visual.

## Anti-goals (a failure even if it works)

- **An animation that fires while the owner is working.** It must never appear
  mid-keystroke, over a fullscreen video, on a locked session or while the
  notch is showing music, lyrics, a notice, a pomodoro or a recording.
- **An animation that cannot be stopped.** Any input ends it within about
  200 ms, by easing home, never by a hard cut that leaves the island bent.
- **A rise in idle CPU.** The round that cut CPU
  ([[bata-idle-power-budget]], [[d2d-dc-target-full-copy]]) is not to be
  undone: nothing new may run while no animation runs.
- **The rejected looks.** A limb that reads as a rope, a snake or a braid; a
  cat body that reads as fat, as bread or as not-a-cat; wings that read as
  thorns; a pop-in "ninja" appearance instead of a physical transition. All
  were rejected by name across seven rounds.
- A notch left deformed, or clock digits left scattered, after an animation
  ends or is interrupted.

## The ten stories

Durations are the mockup's. One mascot is active at a time
(`config.mascot`), so each owner sees only their own three or four.

| Mascot | Story | s | What the island does |
|---|---|---|---|
| Cat | Petak umpet | 7.8 | a bump runs along the lip; it rocks on the pounce (and springs in y on the Pill) |
| Cat | Bantal angka | 8.6 | the lip rises and falls with the cat's breathing; the minute swaps |
| Cat | Piano ekor | 8.4 | digits drop one by one, a small glow under each, a wave at the end |
| Cat | Syal ekor | 8.4 | the left end is squeezed by the tail; a sneeze jolts the island sideways |
| Octopus | Penasaran jam | 7.6 | the "1" is pulled off, wobbles, and is put back |
| Octopus | Peluk notch | 7.8 | the whole notch is squeezed, then springs back |
| Octopus | Bungee | 7.4 | Pill: pulled down and springs back with the rope; Notch: nothing |
| Falk | Kelelawar | 7.6 | Pill: tilts with the bird's swing; Notch: nothing |
| Falk | Maling angka | 7.8 | a digit is carried off in the beak and regrown |
| Falk | Terbang keliling | 7.9 | nothing (it flies out, round and back in) |

The cat's body, the falk's wings and the octopus's long arms are **new
drawings**, not in `src/ui/mascot.rs` today. The heads are the existing ones on
the 64-unit grid and must stay byte-for-byte recognisable.

## Fitting the real island

The mockup was first drawn on a fixed 200 x 30 notch with the mascot at the far
left and the clock at the far right, about 90 units apart. The real idle island
is not that: `geometry::idle_layout_lead` centres **one group**, mascot 26 +
gap 8 + clock 44 = 78 dip, in an island 160-300 wide and 28-44 tall
(`NOTCH_W_MIN`, `NOTCH_W_MAX`, default 190). So in the app the mascot and the
clock sit 8 dip apart in the middle with the ends empty, and that first
choreography did not map onto it.

Round 8 of the mockup fixed both this and the two shapes. Every coordinate in
it is now derived from the island's own edges (left and right end, lip, top
edge, slot, each clock digit) at the real constants, so the file can be read
straight into the painter.

**The notch makes room.** As an animation starts, the mascot eases to the left
end of the lip and the clock eases to the right end, over the same 200 ms as
the wind-down; at the end they ease back to their everyday places. The everyday
notch is untouched, which is the conservative call: nothing moves unless an
animation is running. The mockup's distances become real, and the spreading
apart is itself the first beat of "the mascot comes out to play". Every
timeline is written against the spread layout as a fraction of the island's
width and height, so it holds at 160 dip and at 300.

### The two shapes

| | Notch | Pill |
|---|---|---|
| Position | flush with the screen's top edge | floats `PILL_TOP` = 8 dip below it |
| Ends | 13-radius bottom corners, sides run up to the edge | full half-circles, `corner_radius` = `h / 2` |
| Sides visible | three | four |
| Above it | nothing | an 8 dip gap, too small for a 26 dip mascot |

One **rim function** maps a fraction of the way round the island to a point and
a normal, straight side plus quarter arc on the Notch, half-circle on the Pill.
Every limb that winds round an end is written against it, so each story has one
path, not two tables of points.

Where the shapes differ beyond that:

- **A tail or arm that wraps an end** (Syal ekor, Peluk notch, Petak umpet,
  Penasaran jam) goes round a full half-circle on the Pill. It reads as a
  complete coil instead of catching on a corner, and is **better** there.
- **Deformation needs two pivots.** The Notch must stay welded to the screen
  edge, so it scales and rotates from that edge; the Pill floats, so it uses
  its own centre, and it may also shift in y and tilt, which the Notch cannot.
  Bungee pulls the Pill down and lets it spring back; Kelelawar tilts it with
  the bird's swing. On the Notch both are zero.
- **The clock is no longer still.** Because the Pill itself can shift and tilt,
  the clock's transform follows the island, not the screen.
- **The mascot in the slot needs an island-shaped clip** on the Pill, where the
  rounded end cuts the corner of its head. On the Notch it is nearly a no-op.
- **Terbang keliling is the one story the Pill does nothing for.** The 8 dip gap
  is too small to fly over, and the hiding place behind a rounded end is a
  little smaller, so the bird is out of sight two or three frames less. Nothing
  breaks; it simply gains nothing.
- **Maling angka clips the falk's crest** against the Pill's top edge when it is
  startled: 3 to 5 dip at 34 tall, gone at 44. On the Notch the crest passes the
  screen edge and is hidden by the bezel, which looks better.
- **At 160 x 28 both shapes are tight**: a 26 dip mascot is nearly as tall as
  the island, so cat ears and the falk crest clip a little, slightly more on the
  Pill. That is as cramped as the real app already is at that size.

**Coats and accessories.** The mockup draws one coat per mascot. The app has
four cat coats, three falk, four octopus, and ten accessories
([[cat-colours-and-surfaces]], [[cat-accessories]], [[maskot-falk-gurita]]).
The new body, legs, tail, wings and arms take their colours from the same
`theme::Coat` derivation the head already uses, including the calico patches
and the black cat's rim, and any accessory stays on and follows the head
through the whole animation. The owner's cat coat is Calico, which has patches,
so this is not theoretical.

**With the clock off** (`idle_clock: false`) the four stories that play with
the digits (Bantal angka, Piano ekor, Penasaran jam, Maling angka) drop out of
the pool and the hourly moment is skipped. Every mascot still keeps at least
two stories, so no pool is ever empty.

**With a see-through island** (`notch_fill` not Solid) the animations still
play. Nothing may rely on the island being opaque to hide a limb or a body.
The paint order the mockup proved, at 42% fill with nothing leaking:

1. everything **behind** the island, clipped to "screen minus island"
   (even-odd), so a hidden part is cut away rather than covered;
2. the parts meant to read as **inside** the dark, clipped to the island
   shape;
3. the island's own fill and bezel, last.

**The animation must not swallow clicks.** Today nothing is drawn outside the
island's own shape, and the layered window takes mouse input wherever a pixel
has alpha. An animation paints up to about 110 dip to the side and 70 dip below
the notch, so for eight seconds it would eat every click in that area into
`Under::Off` and do nothing with it, which is the "never gets in the way"
anti-goal exactly. So: a point inside an animation's pixels but outside the
real island shape is **transparent to the mouse** (the island window gains the
hit rule it does not have today), the click reaches whatever is underneath, and
the animation winds down.

## Approaches

**A. Manual only (right-click, nothing automatic).** One menu item, no timer,
no gating, no risk. Rejected as the whole answer: the owner would almost never
see ten animations that were worth seven rounds of design, and a thing you must
summon stops feeling alive. It is kept as one of the triggers.

**B. A plain random timer.** A one-shot that fires every N minutes and plays.
Simple. Rejected: it fires while the owner is typing, while they are away from
the PC, and at moments with no meaning, which is exactly the "gets in the way"
anti-goal.

**C. One budget, several moments, with a presence rule (picked).** An animation
may play only when the owner is demonstrably present but not busy, at most once
per cooldown, and the moment chooses the story. More logic than B, but every
piece of it is a pure function that can be unit tested, and it is the only
option that satisfies the anti-goals. The owner delegated this choice
("u atur yang terbaik").

## The trigger

### The presence rule

The heart of it. An automatic animation needs the **last PC input to be
between 15 seconds and 5 minutes ago**.

- Under 15 s: the owner is typing or clicking. Stay out of the way.
- Over 5 minutes: they have left the desk; the animation would be wasted, and
  the cat is about to fall asleep anyway (`robot_sleep_minutes`, default 10).
- In between: they are reading or thinking, at the screen, with their hands
  still. That is the moment to be seen.

`GetLastInputInfo` is already read on every `refresh` and every `TIMER_ROBOT`
fire (`robot::fold_input`, `App::fold_pc_input`, [[bata-idle-power-budget]]),
so this costs nothing new.

### The gates

Every one of these must hold, for every automatic play:

- the mascot is on, `Motion` is **Full**, Windows' Energy Saver is off,
  Windows animations are on;
- the closed **idle** view is showing: no media, no lyric, no notice, no
  pomodoro, no CLI or Claude face, no recording, no captions;
- the panel is closed and not lingering, no menu or popover is open;
- not fullscreen, the session is not locked, the display is on
  (`screen_away`);
- the notch is not shrunk to the eyes, and the pointer is not within the
  notch's hover zone (hover already opens the panel, [[open-mode]], and the
  peek already owns that zone, [[notch-peek-eyes]]);
- the mascot is not asleep;
- the cooldown has passed.

### The three automatic moments

All three share **one cooldown**, so they never stack:

1. **Iseng.** A one-shot timer at a random wait. The everyday case.
2. **Jam bulat.** At the top of the hour, prefer a story that plays with the
   clock (cat: Piano ekor or Bantal angka; octopus: Penasaran jam; falk:
   Maling angka). Every mascot has at least one, so the hour always has a
   candidate. It rides the existing `TIMER_CLOCK`; no new timer.
3. **Sapaan.** The greeting already fires at most once per period per day, at
   startup, wake and unlock, with no timer of its own ([[greeting-periods]]).
   But the greeting **is** a notice, and it fires just after the owner touched
   the PC, so it fails both the "no notice" gate and the presence rule. It is
   therefore its own case: the warm story (cat: Syal ekor; octopus: Peluk
   notch; falk: Terbang keliling) plays **when the greeting notice ends**,
   with every gate rechecked at that moment except the presence rule, which
   the greeting itself stands in for. The notice's deadline already rides
   `TIMER_DEADLINE`, so no new timer is needed: `TIMER_MASCOT_PLAY` is armed
   for the moment the notice closes.

Otherwise the story is a weighted random pick from the active mascot's pool,
never the one just played.

### Right-click: the owner's trigger

A right-click on the island already opens the tray menu
(`RightClick::Tray`, `src/app.rs`), and the code already special-cases a
right-click by what it is on (`note_card_menu_target`, `rec_row_menu_target`).
So the gesture is taken, but the menu it opens is the right home:

**A new `MenuCmd::PlayMascot` in `src/ui/tray.rs`** ("Mainkan maskot" /
"Play the mascot"), near the top of the menu, which plays one at once.

- It ignores the cooldown, the presence rule and the idle-moment gates: the
  owner asked for it, so it happens.
- It still respects the hard ones: it is hidden (not greyed, like the menu's
  other context-dependent items) while `Motion` is **Mati**, while the mascot
  is off, while a recording, a fullscreen window or a lock makes drawing
  wrong, and while the notch is busy with media or a notice.
- **It stays in the menu while the panel is open.** With `open_mode: Hover`,
  the default, the pointer opens the panel before the right-click lands, so a
  rule that hid the item behind an open panel would hide it from most people
  for ever. Choosing it closes the panel and plays. The owner's own config is
  `Click`, where the notch is still closed at right-click time, but the item
  must work in all three modes ([[open-mode]]).
- `COMMANDS` grows from 14 to 15; `src/ui/tray.rs`'s own tests cover the ids.

This replaces the "a click cannot be the trigger" finding from brainstorming:
a **right**-click can, through the menu it already opens.

### How often

A new config key, `mascot_play`, read leniently, default `sesekali`:

| Setting | Wait between iseng plays | Cooldown (any reason) |
|---|---|---|
| Sering | 8-15 min | 6 min |
| **Sesekali** (default) | 15-35 min | 12 min |
| Jarang | 45-90 min | 40 min |
| Hanya sapaan | never | 12 min (greeting and the hour only) |
| Mati | never | - (nothing automatic) |

**Mati** turns off the automatic plays only; the right-click item stays. The
owner asking for one by hand is a different thing from the app deciding to show
one. The global `Motion: Mati` (`robot_animation`) remains the real off switch
and removes the menu item too.

Settings -> **Maskot** gains one row, "Animasi maskot otomatis", with those
five choices and a one-line description ([[settings-structure]],
[[settings-spacing]]).

## Yielding

An animation is never cut. On any of: pointer entering the notch zone, a click,
a new notice, media starting, the panel opening, a hotkey, fullscreen, lock or
display off, the animation enters a **wind-down**: the mascot eases back to its
idle slot pose and the island's deformation unwinds to zero, both over 200 ms,
then the normal view resumes. The same wind-down runs when the animation ends
normally, so there is exactly one way home and the island can never be left
bent.

With Windows animations off or `Motion` not Full mid-animation, the wind-down
is instant (one draw), as elsewhere in the app.

## Drawing (Direct2D)

From the mockup's own notes, confirmed against the painter code:

- **The island outline**, today one path, is sampled to about 90 points and
  rebuilt per frame while a dent, a bulge or a squeeze is on. No effects, no
  blur: a rebuilt path. A sneeze is a short x shift. A swing is a rotation,
  about the screen edge on the Notch (with the top extended behind it so no
  hole shows) and about its own centre on the Pill.
- **One shared rim function** (a fraction round the island to a point and a
  normal: straight side plus quarter arc on the Notch, half-circle on the
  Pill) carries every limb that winds round an end. Without it each story
  would need two point tables.
- **The inverse clip** ("screen minus island", a geometry group with
  `D2D1_FILL_MODE_ALTERNATE`, or an inverted layer) wraps every back layer,
  and is rebuilt each frame while the island is deforming.
- **The mascot in its slot is clipped to the island shape** so the Pill's
  rounded end cuts the corner of its head. Nearly a no-op on the Notch.
- **The clock's transform follows the island**, since the Pill can shift in y
  and tilt. It can no longer be treated as fixed to the screen.
- **The clock digits must be drawn per character** (move, rotate, scale, fade,
  fall over, swap the minute as two crossing texts). Today the clock is one
  tracked DirectWrite run in a `CLOCK_W` box. This is the single biggest
  change outside the mascot art, and it sits on a path that draws all day, so:
  the per-character positions are **measured from DirectWrite's own cluster
  metrics**, never from the mockup's constants, and the resting clock stays
  pixel-identical to today's. The per-character path runs only while an
  animation runs, and the handoff into and out of it changes no pixel; a test
  compares the two renderings of the same time.
- **A limb that passes in front of the island** (the tail scarf, the octopus
  arms) is drawn a second time, clipped to the island shape: one layer clip per
  frame, the same trick for all of them.
- **Feathers** are rounded-tip capsules (quadratic curves plus
  `ID2D1GeometrySink::AddArc`); a spread wing is about 20 small paths, so about
  40 with two wings open. The wing's tip uses the pose from about 70 ms earlier
  so it lags the shoulder: arithmetic, not a timer.
- **The cat body** is a few fixed paths (body, side shade, two haunches) plus
  short tubes for legs, paws and tail, all on the head's grid inside one
  transform, so the head and body can never come apart.
- The piano glow is a small translucent ellipse; falling feathers use opacity.

The clamp rule of [[maskot-falk-gurita]] applies to anything inside the island,
but these animations deliberately draw **outside** it, in the 620 x 330 dip
window, which already exists. The dirty-rect plan ([[bata-idle-power-budget]],
"Dirty-rect present") must treat a running animation as a **Full** frame, like
the other morphs, since the painted bounds move far outside the notch.

## Timers and power

- One new one-shot id, `TIMER_MASCOT_PLAY`. Id 19 looks free; the builder
  confirms with `app::tests::timer_ids_never_collide`.
- It is armed only when every gate above holds, and killed the moment one
  stops holding, exactly like `TIMER_TYPING` (`sync_typing_timer`). A new
  `sync_mascot_timer`, called from `sync_robot_timers`, `sync_pointer_polls`
  and `follow_pointer_settings`, owns it.
- While an animation runs, the existing `FrameClock` runs, counted in
  `App::animating_at`, and stops when the wind-down ends.
- Nothing runs while no animation runs. An idle island costs what it costs
  today.

## What is left to the builder

- The exact story-to-moment weighting, beyond the clock and greeting
  preferences named above.
- Which module the stories live in. A `src/ui/mascot_play/` with one pure
  timeline module (times, poses, deformation amounts; unit tested) and one
  painter module matches how the rest of the app is split.
- The 1:1 legibility pass: the mockup's own weak points are recorded in its
  notes (the peeking head is small at real size, the cat covers part of the
  clock in Bantal angka, the squeeze is subtle in Syal ekor). Tune at 28 and
  44 dip notch heights.
- Demo scenes: `--demo-robot` gains one scene per story, and `DI_MASCOT`
  picks the mascot, as in [[maskot-falk-gurita]]. Each scene must be
  reachable in both styles and at the extreme sizes, since that is where the
  mockup's own weak spots live.
- Whether the stage for the three clock close-ups (Bantal angka, Piano ekor,
  Penasaran jam) shows the "island makes room" beat in full. The mockup keeps
  them zoomed in, so that beat is only half visible there; in the app the
  whole island is always on screen, so the question does not arise, but it
  means those three were judged by the owner without seeing that beat.
- Tests: the gate and cooldown logic, the presence window, the story picker
  (never twice in a row, clock stories at the hour), the wind-down always
  returning the notch to zero, and the timer-id collision test.

Related: [[maskot-falk-gurita]], [[bata-idle-power-budget]],
[[bata-robot-expressions]], [[greeting-periods]], [[notch-peek-eyes]],
[[open-mode]], [[settings-structure]], [[cat-dances-to-music]],
[[cat-types-while-working]]
