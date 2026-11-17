---
tags: [decision, notch, theme, settings, mascot]
date: 2026-11-17
---

# Warna notch: Polos, Gradasi, Jadi muka

Owner request (2026-11-17): the notch should carry the chosen mascot's colour as
identity, not as a status signal. After three mockups (`docs/design/warna-notch.html`,
`warna-notch-maskot.html`, `notch-muka.html`) the owner kept two styles as a
setting and left the default plain. Spec:
`docs/superpowers/specs/2026-11-17-warna-notch-design.md`.

## The setting

- Settings -> Tampilan -> Island, row **Warna notch** right after Warna dasar:
  Polos | Gradasi | Jadi muka (EN Plain | Gradient | Face). The description names
  the style and the coat it takes its colour from ("Gradasi · dari Terracotta").
- Config `notch_style: NotchStyle { Plain, Gradient, Face }`, default `Plain`,
  lenient (`named_enum!`: a typo or a missing key reads as Plain, old files do not
  change), island-owned (`merge_owned`), exported (`config_transfer.rs` `EXPORTED`).
- Polos is today's notch: same solid brush, same palette, no new draw.
- Maskot di notch off: the Jadi muka segment is faded and takes no clicks
  (`segment_alpha`, per-item `enabled`). The note "Nyalakan maskot dulu" replaces
  the description only when Face is the stored value; Polos and Gradasi keep their
  own text (Gradasi then says "dari <accent>"). `notch_colour::effective_style`
  draws a stored Face as Gradasi (accent tint) and does not rewrite the stored value.
- Under effective Face the "Isi notch" segment and "Opasitas" slider are faded
  (note "Jadi muka selalu solid").
- Demo variable `DI_NOTCH_STYLE=gradient|face` for every `--demo` scene and `--settings`.

## Colour rules: `src/ui/notch_colour.rs`

Pure functions, no drawing.

- `tint(look, robot_on, accent)`: one colour per coat (Terracotta and Calico
  #E2683C, White #F6F1EA, Black #8E7CB8 from its rim, falk Lutino #F6D04A, falk
  Abu-abu and Muka putih #9A959C, octopus its body colour). With the mascot off it
  is the accent.
- `bottom(surface, tint, max_t, accent)`: the surface blended toward the tint by
  the largest `t <= max_t` where cream `CREAM` #F4EDE4 holds 5:1 and the caption
  colour holds 4.5:1. Caps: `CLOSED_T` 0.45 (closed views, notices), `PANEL_T`
  0.32 (open panel). With `Some(accent)` (notices and panel) `t` is also capped so
  the accent holds 3:1. Pale coats blend less automatically (`pale_coats_blend_less`
  compares `t`, not luminance).
- `TOP_STOP` 0.28: the gradient stays at the resolved Warna dasar surface down to
  28 % of the height, so the top still merges with a black bezel.
- `face_fill(look, surface)` is `peek_face(look, surface).fill`, so the notch and the
  shrunk pill always agree ([[peek-mascot-face]]).
- `face_palette(base, fill)`: text and on_accent are `INK` #17120F where it holds
  4.5:1 on the face, else cream; secondary is mixed toward the face up to 0.30 and
  held at 4.5:1; accent, sage, ochre and plum fall back to the text colour under
  3:1. Black cat keeps cream; red octopus (#D9505F) gets `INK`.
- `gradient_palette(base, bottom)`: only the accent changes (to text under 3:1).
- `needs_ring(mark, fill)` (REC dot under 3:1) and `needs_halo(head, fill, has_rim)`
  (head under 3:1 and no rim).
- Tests run every coat x the six surfaces x both caps (cream 5:1, caption 4.5:1,
  accent 3:1) and every face fill (ink 4.5:1, marks 3:1).

## Renderer (Gradasi)

- `IslandFill { Solid { rgb, alpha }, Gradient { top, bottom, alpha } }` from
  `Renderer::island_fill` (rest) and `island_fill_at(frame, panel_share)` (morph).
  `paint_island` is still the one fill site. Plain keeps the cached solid brush.
- Stops `(0, top) (TOP_STOP, top) (1, bottom)`, each at `fill_alpha`, so Tembus
  still works. The gradient brush is built over the shape's y-range and cached
  (key: height, stops, alpha).
- `panel_share` comes from the eased shape height between the closed view's
  `fade_floor_h` and the panel's rest height, so the panel bottom mixes
  closed -> panel with the same progress as the fill morph.
- Palette override: `Renderer::content_palette(frame)` applies `gradient_palette`
  to the closed views and notices after the panel early return; the closing panel
  keeps its own palette.
- Halo: `Renderer.notch_grad` (a `Cell`, set only while closed content paints
  under Gradasi) lets `paint_robot_at` draw the existing dark halo at full strength
  when `needs_halo`. The Black cat keeps its rim instead.
- REC dot and the screen countdown ring: a 1.5 dip ring in the text colour when
  `needs_ring` against the fill under them (`mark_needs_ring`). The panel REC dots
  sit on unchanged `p.raised` cards and have no ring. The unread toast dot gets the
  same check under Gradasi and Face.
- Dirty rect: `Frame.notch_style` and `notch_tint` are in `Drawn` and `EqKey`
  (`ui/dirty.rs`). No new timer, thread or clock ([[bata-idle-power-budget]]).

## Jadi muka

- Closed compact views only: Idle, MediaCompact, MediaLyric, PomodoroCompact, the
  recording and caption views and `ScreenCountdown`. Notices, the greeting and
  the open panel use Gradasi.
- Fill is `face_fill`, always opaque. Face palette applies to those views only.
- Painter (`robot.rs`): `paint_face` draws marks only (eyes in every expression,
  cheeks, beak, calico patches clipped by the notch, octopus spots, z's and heart
  beside the eyes, never above). `face_marks`, `face_coat`, `face_slide`,
  `face_boxes`, `face_pose_in` (clamps the look like `eyes_offset_in`; both share
  `reach`). The look slides the eyes only; the beat nods the whole face. Typing
  draws the Focus eyes lower (`TYPING_FACE_DOWN`).
- Geometry (`geometry.rs`): `face_area(shape, clock_left)` and
  `face_scale(area, accessory)`. `FACE_SCALE` 1.6 (x the shrunk pill) holds on the
  default 190 x 34 idle notch; `face_scale` shrinks it where it cannot fit.
- Face-mode shapes in narrow views (media, lyric, pomodoro, REC) are widened by
  +12 dip so the face keeps scale >= 1.0 and the shrink to the eyes still goes
  down. Only Face-mode shapes change.
- Idle face and clock are centred as one group (like `idle_layout_lead`).
- Accessories (`cat_style.rs`: `FacePiece`, `FaceAnchor`, `face_pieces`,
  `face_half`, `paint_face_pieces`): glasses on the eyes; earcups at the notch
  ends (no band); small bow tie under the eyes; scarf along the bottom edge;
  bow and flower in the notch's left corner, Idle only. The three hats
  (Nightcap, Crown, Sailor) are brims along the top edge over the face
  (`FaceAnchor::Rim`, `rim_pieces`), on every Face view: the owner first picked
  "miring di pojok", then switched to "pinggiran" the same day. The nightcap tip
  and pompom and the sailor's tails are left off in the narrow views (no room);
  the brim keeps clear of every eye box. Narrow views wear glasses, bow tie,
  earcups and the hat brims; the scarf is Idle only. With the Headphone on, the
  unread toast dot moves inward past the right earcup (`face_toast_dot`). A piece under 3:1 against the face gets a
  1 dip edge in the eye colour; the crown always has `CROWN_EDGE` #9A6A12.
  Pale faces get the 1 dip surface edge, the Black cat its rim, both on the
  outline.
- Plays (mascot stories) are off while the effective style is Face
  (`play/gate.rs` `permanent_ok`).
- `peek_mix` stays 1 under Face. The shrink to the eyes slides and scales the face
  from its notch place and scale to the pill's (`paint_face_to_pill`, and
  `paint_face_from_pill` back) on the morph's content fade; the clock and
  accessories fade, and in the last 20 % the pill's own marks take over. The pale
  edge and the Black cat's rim are drawn with the fill, so they never dip.
  `PeekMix::retarget_at` keeps the mix continuous when the target changes mid-morph.
- `face_last` (the face to fade out) is forgotten on Hidden, a settled pill, a
  settled notice or panel; leaving Face (style switch, mascot off) fades the face
  out over the morph (`leaving_face`). No ghost face.
- REC level meters under Face use the face ink where their colour is under 3:1
  (`level_tint`); the mute pill keeps its own dark plate.
- Face <-> Gradasi morph (notice arrives, panel opens): the fill cross-fades, the
  marks fade out, ink switches where cream reaches 4.5:1 on the blended fill.

## Accessories move with the face (2026-11-18)

Owner, after the first hand test: "accesorisnya dibikin ngikut gerak juga dong".
Spec `docs/superpowers/specs/2026-11-18-face-accessories-follow-design.md`.

- Every worn piece moves by the face's look slide and nod (`FacePiece::place`
  applies both to every anchor); blink stays on the eyes.
- `face_pose_in(.., kind)` clamps one group: the face marks plus the whole-object
  pieces (Glasses, Bow tie, Bow, Flower). Edge-hugging pieces (`hugs_edge`: hat
  brims with the nightcap tip and sailor tails, Scarf band and tail, Headphone
  earcups) are left out of the clamp and drawn clipped to the island outline, so
  at full look an earcup or a scarf end slips behind the notch edge like a turning
  head. A single clamp over every piece had cut the look to under 1 dip with the
  scarf (ruling, ledger). With no whole-object piece the face keeps its full
  4 dip look and 1.5 dip nod on 190 x 34.
- The scarf runs past both ends (`face_reach_x`) so a slide never shows a gap; hat
  brims extend above the top edge so a nod opens no gap under them; `RIM_PAST` is
  1.5 so the brim stays left of the clock at the full typing glance.
- The pale faces' edge and the Black cat's rim are stroked again after the pieces
  (`stroke_face_edge`, the preview's `edge_line`), in the base palette's colour.
- With the Headphone, the unread toast dot clears the earcup at its furthest-in
  position; narrow views end their content ~5.3 dip earlier and the idle
  face + clock group moves up to ~4 dip left on notches under 170 (Face +
  Headphone only).
- Not looked at on screen: the earcup gap of up to ~8.6 dip on the far side at
  full glance, the clipped earcup and scarf ends, the preview's re-stroke.

## Rulings made while building

- Mascot off: the note "Nyalakan maskot dulu" only when Face is stored (first
  ruled for every style, reversed after the final review: a Polos user with the
  mascot off must not read the setting as broken).
- Narrow views: widen Face-mode shapes instead of a face scale under 1.0.
- Bow and flower Idle-only; the scarf Idle-only (it would cross text elsewhere);
  hat brims on every Face view.
- Notices and panel cap the tint so the accent holds 3:1 (no audit of panel painters).
- Panel REC dots get no ring.
- Clock outline during plays, the peek edge, the dock pill and demo cards still use
  the flat surface under Gradasi (visual check pending).

## Not verified live

`cargo test --locked` green on `main` after the merge; the debug `--demo` ran
`DI_NOTCH_STYLE=gradient` and `face` over every compact scene, Pill and Notch,
for cat, falk and octopus, with no draw error. Nothing was judged by eye: each
coat in Gradasi and Jadi muka on a real screen, bright and dark wallpapers and a
laptop bezel, the real pointer look and shrink to the eyes (calico patches handing
over to the pill's dots), the Face <-> Gradasi cross-fade, hat brims at 160 x 28,
the clock outline during plays, the peek edge and the dock pill (still the flat
surface under Gradasi), the Settings previews.

Related: [[notch-size-and-fill]], [[peek-mascot-face]], [[cat-colours-and-surfaces]],
[[cat-accessories]], [[notch-peek-eyes]], [[bata-idle-power-budget]],
[[settings-structure]].
