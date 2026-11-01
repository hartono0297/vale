---
tags: [decision, cat, robot, painter, geometry, motion, power]
date: 2026-11-01
---

# The cat types on a laptop while work runs

Owner report: "after the music ends the cat goes idle and does not follow the
cursor". It was not Idle: it was `Expr::Think` (a Claude Code session active in
the last 2 minutes), and Think never looks. The owner chose a typing cat
instead (variant "B1"): while work runs the idle cat types on a small laptop
and only its eyes glance at the pointer. Faces and priorities:
[[bata-robot-expressions]]; the pointer rules: [[cat-looks-at-pointer]]; the
cost: [[bata-idle-power-budget]].

## The face

- `Expr::Typing` is the 11th face (`Expr::ALL`, i18n `ExprTyping`
  "Mengetik" / "Typing"). `choose_expr` in the idle branch gives it for
  `claude_working || cli == Running || mole_running` (CLI Done/Failed, hover's
  Happy, media, lyric, pomodoro and notices are unchanged). **Think stays in the
  enum** (demo sheet, the `bata-claude` notice scene) but is never chosen.
- `typing_wanted(inputs)`: `choose_expr` with `hover: false` gives Typing. The
  island's shape uses it, so hovering (Happy) does not shrink it under the
  pointer.
- `blinks()` true, `looks()` false (the head and body never move), `glances()`
  true only for Typing, `follows() = looks() || glances()`.
- `Face::eye_shift` (grid units, (0, 0) elsewhere) shifts every face mark
  before the tilt. Typing: the idle pills `open(10.0, 5.6, 0.0)`, shift
  (-3.5, 1.0), so the centres are (20.5, 41) and (36.5, 41); blink gives the
  Blink lines shifted the same way; ears at rest, bob 0, tilt -4 degrees (the
  existing paint rotates the whole cat, rim, ears, head, patches, marks and
  accessories, around (32, 40)). The glasses ride the eyes matrix, so they
  follow the shift and the glance.

## Laptop and paws (grid units, fixed colours, not the coat)

Drawn only for Typing, in the cat's grid scaled by `k` and placed like the cat
(`place`: lift, bounce, head slide, nod) but **not** rotated by the face tilt.

| Piece | Geometry | Colour |
|---|---|---|
| base | rounded rect x -27..9, y 53..56.4, r 1.5 | #8F857C |
| lid | polygon (-23,54) (-17.5,54) (-20.5,27.5) (-25.6,28.2) | #4A423C |
| lit edge | polygon (-20.5,27.5) (-18.4,27.3) (-15.3,53.6) (-17.5,54) | #9CC8E8 at 0.55 |
| logo dot | circle r 1.5 at (-21.6, 40.5) | #F4EDE4 at 0.85 |
| paws | ellipses rx 3.8, ry 2.4 at (1, 52.6) and (9.5, 52.6), raised by `paw_lift` | the coat's head colour; Black only: 1.0 outline in the rim colour |

Paint order in `RobotPainter::paint`: laptop first, then the cat exactly as
before (rim, silhouette, calico patches, marks, accessories), then the paws.
The two polygons are paths built once (`Shape::Lid`, `Shape::LidEdge`).
`laptop_rect(area)` is the laptop's dip box (grid x -27..9, y 27.3..56.4); the
see-through legibility halo in `render.rs::paint_robot` draws the same two
steps round it (radius 1.5 k + grow).

## Pose

`Pose` gained `glance_x`, `glance_y` (grid units, added to the face marks'
translation only) and `paw_lift: [f32; 2]` (left, right). Constants
`GLANCE_X` 4.0, `GLANCE_Y` 1.5, `PAW_LIFT` 1.6. `glance_offset(vx, vy)` clamps
and treats NaN as 0; `Pose::glancing` sets the glance only (no head slide or
tilt).

## The paws' rhythm (`TypingPaws`, pure)

`next(&mut rng) -> ms`: the paws take turns (left up, then right up,
alternating, across a pause too). A normal step waits 110..=200 ms
(`TYPE_STEP_MIN_MS`, `TYPE_STEP_MAX_MS`); `TYPE_GAP_PCT` = 12 % of the steps
wait `TYPE_GAP_MS` = 420 instead; from a paw up, one step in
`TYPE_PAUSE_ONE_IN` = 43 starts a pause: both paws down for `TYPE_PAUSE_MS` =
900. The pause is only rolled from a paw-up state, so pauses never chain: the
mean time between pause starts is about 8.9 s (the test allows 8 s +- 2).
`lift()` is `PAW_LIFT` for the paw that is up, `reset()` puts both down.
Seeded 20 000-step test: every wait is 110..=200, 420 or 900, the 420 share
is 12 % +- 3, paws alternate, both are down in a pause.

## The gate (`typing_anim_on`)

True only with: expr == Typing, `motion.moves()` (the effective motion: Saver,
Off and Full-with-Energy-Saver are out), Windows animations on, not
fullscreen, not `screen_away` (locked or display off), not shrunk. Anything
else: paws down, eyes on the screen (`Brain::pose` is still anyway), no timer.

## The timer id

`TIMER_TYPING` = **40**: the next free id (`TIMER_UPDATE` is 35, the notification
history's is 34). It is a one-shot re-armed with each step's wait, one `draw`
per step unless the frame clock draws anyway. It could not share
`TIMER_BLINK` or `TIMER_ROBOT`, which already carry the cat's blink, the
shrunk eyes' blink and `Brain::next_change`: any of them stopping would stop
the paws. `timer_ids_never_collide` has it (and asserts 40); if another
branch takes 40 first, the test says so and this one moves to the next free
id. `App::sync_typing_timer` arms or kills it from `sync_robot_timers` (so
every `refresh`: Claude activity, a CLI start or finish, a Mole start or
finish, `TIMER_ROBOT`, fullscreen, Energy Saver and the Windows animation
setting all reach `refresh`), from `sync_pointer_polls` (lock, display) and
from `follow_pointer_settings` (a config adoption). The WM_TIMER handler kills
and returns when the gate is closed.

## The shape: grown on the left

Assumption: the owner asked for the island to grow "on the left", so the robot
and the clock stay exactly where they are on screen.

- `TYPING_LEAD` = 12 dip (the laptop reaches about 11 dip left of the 26 dip
  robot square). `idle_shape_typing(style, size, robot)` is the idle shape with
  x reduced and w increased by it (the right edge stays).
- `typing_lead(shape_w, notch_w)` = `(shape_w - notch_w).clamp(0, 12)` (NaN 0),
  so it follows the morph. `idle_layout_lead(s, robot, clock, lead)` centres
  the group in the shape minus `lead` on its left: `x = s.x + lead + (s.w -
  lead - group) / 2`. `idle_layout` is lead 0 (the Settings preview is
  unchanged). With the lead from the shape, the robot and the clock keep the
  same screen x through the whole grow and shrink.
- `App::idle_shape` (one helper) is used for the idle target in `refresh` and
  for `normal_shape` (the peek). It grows and shrinks with the usual compact
  morph (`view_same`, no fade). `Frame::idle_lead` carries the lead to the
  renderer (`typing_lead(shape.w, notch.w)` for View::Idle with the cat on).

## Gotchas

- The left growth is not needed to fit the laptop (the 160 dip notch already
  has room left of the group); it is the owner's design. The clamp test
  therefore checks fit, not necessity: it fails when the laptop is moved far
  left (checked by hand with the base at x -150).
- The face sheet had 10 faces in two rows of five; with 11 faces it is six to
  a row, the typing cat at the right of its cell so its laptop does not touch
  the neighbour.
- `the_typing_cat_stays_in_the_idle_island` (geometry.rs, next to the looking
  one): both styles, every notch size 160..300 x 28..44, clock on/off, the
  Typing face open and blinking, at the glance extremes ((+-1, +-1), (0, 0))
  and paws [0,0], [1.6,0], [0,1.6]: head, ears with their round stroke under
  the -4 tilt, the Black rim, every accessory (`accessory_corners`), every
  eye box, the laptop pieces and the paws (outline included, untilted via
  `drawn_flat`) inside `shape_contains` and left of the clock box.

## Demo (debug only, no real data)

`bata-cli-running` is now Typing; `bata-ketik` (paws down, eyes on the
screen), `bata-ketik-paw` (left paw up), `bata-ketik-lirik-kiri` and
`bata-ketik-lirik-kanan` (glance (-1, 0.3) and (1, 0.3)): all idle scenes with
the typing shape and lead. Scene fields `glance` and `paws`. Coats,
accessories, size and style come from `DI_CAT_COLOR`, `DI_CAT_ACCESSORY`,
`DI_NOTCH` and the Pill/Notch steps.

## Not verified live

Everything above is pure maths, geometry tests and a release build. Not seen
on the real island: the typing rhythm, the glance with a real pointer, and the
left-side growth when a Claude session starts and stops (the morph's feel and
the laptop's legibility on a see-through notch).

Related: [[bata-robot-expressions]], [[cat-looks-at-pointer]],
[[bata-idle-power-budget]], [[cat-accessories]], [[cat-colours-and-surfaces]],
[[cat-dances-to-music]], [[notch-size-and-fill]], [[exe-size]]

Listed in [[index]].
