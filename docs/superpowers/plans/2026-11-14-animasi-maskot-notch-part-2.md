# Mascot stories around the island, part 2: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The other nine stories (four cat, three octopus, two falk) play around the island, in both styles, as the owner approved them in the mockup.

**Architecture:** Part 1 built the engine (`src/ui/play/`: rim, `Deform`, `Beat`, triggers, gates, `stories/`), the per-character clock, the app timer, click-through, the menu item and the Settings row, and one story drawn only *inside* the island. Part 2 adds the mockup's full layer stack (behind the island, inside it, wrapped on its face, in front), a small set of pure drawables the renderer paints, three pieces of new art (the seated cat's body and tail, the octopus's long arms, the falk's wings and body) on the heads' 64-unit grid, and then one timeline module per story. All art and timelines stay pure and unit-tested; `render.rs` only turns drawables into Direct2D calls.

**Tech Stack:** Rust 2024, `windows` crate (Direct2D, DirectWrite), Win32 layered window. Windows 11 x64.

**Spec:** `docs/superpowers/specs/2026-11-13-animasi-maskot-notch-design.md` (the binding authority; "The ten stories", "Fitting the real island", "Drawing (Direct2D)"). Visual reference, owner-approved: `docs/design/maskot-notch-v3.html` (story ids `c1`-`c4`, `o1`-`o3`, `f1`, `f3`; serve it over http to use the `#sheet=<id>:t1,t2[;pill][;WxH]` hook). Part 1 plan: `docs/superpowers/plans/2026-11-13-animasi-maskot-notch.md`. Memory: `docs/memory/mascot-play.md`, `play-click-through.md`, `maskot-falk-gurita.md`, `bata-idle-power-budget.md`.

## Global Constraints

- Every timeline, easing and coordinate comes from the mockup story, derived from the island's own edges (`rim`, lip, ends, slot, each clock digit) as the mockup derives them. Where the mockup and a task's text disagree, the mockup wins (as in part 1's R44).
- Both styles (Notch, Pill) and the sizes 160x28, 190x34 (default) and 300x44 must work; nothing leaves the 620 x 330 dip window (`WINDOW_W`, `WINDOW_H`).
- Nothing may rely on the island being opaque: with a see-through fill, a part that should be hidden is cut away by a clip, never covered.
- The heads are the existing painters (`RobotPainter::paint`), byte-for-byte recognisable. New body parts take their colours from the same coat derivation the head uses (`cat_style::CatStyle`, `mascot::falk_ink`, `mascot::octopus_rgb` / `shade` / `sucker_rgb`): four cat coats including Calico's patches and Black's rim, three falk coats, four octopus coats. The chosen accessory stays on and follows the head for the whole story.
- A story starts and ends exactly at rest (island, clock, mascot), so it never pops; the wind-down eases every part home with `beat.alpha`.
- Idle power budget: no new timer, no thread, nothing running when no story plays; the per-frame cost exists only while a play runs.
- Never run `cargo fmt` (the repo is not rustfmt-clean). Run `rm -rf target/test-tmp` after a full test run.
- Copy is Indonesian with English counterparts in `src/i18n.rs` where any user-facing text is added (none is expected).

## Review Focus

1. **See-through fill**: a limb hidden "behind" the island must not show through a 42 % fill, at any moment of any story. Task 1 adds a paint test over the real renderer; every story's demo check includes the see-through fill.
2. **Calico and Black on the new body**: the owner's cat coat is Calico; patches on the body and the Black cat's rim are where a coat bug would show. Task 4's tests cover all four coats; the controller's capture includes Calico and Black.
3. **The accessory leaving the slot**: a head drawn in the back or front layer (outside the island) still carries its accessory. Task 1's head drawable goes through the same painter call as the slot head; Task 4 tests a head placed outside the island with an accessory.
4. **Clock off**: with `idle_clock: false` the four digit stories drop out; every mascot must keep at least two stories. Task 15 adds the test over the real pool.
5. **160 x 28**: the tightest island, where ears, crest and tails clip. Every story's window-bounds and rest tests run at 160x28, and every demo capture includes it.

---

## Decision carried over from part 1 (for the owner's review of this plan)

Part 1's visual check clipped Maling angka's falling digit to the island. The mockup does not: its `front` layer is unclipped, and the digit is outlined in the island's colour precisely so it reads off the island. Part 2 builds that `front` layer, so **Task 1 moves the digit and the whistle back to `front`, as the mockup draws them**: the stolen digit falls out under the island and fades. If the owner prefers the clipped version, Task 1 leaves them in `dark` instead; nothing else depends on it.

---

### Task 1: The layer stack and the drawables

**Files:**
- Modify: `src/ui/play/stories/mod.rs` (Shot, Part, Layer, Draw), `src/ui/play/stories/maling_angka.rs` (emit parts), `src/ui/render.rs` (compose the layers, paint the drawables)

**Interfaces:**
- Consumes: `Shot`, `MascotShot`, `Prop`, `island_geometry`, `push_mask`, `paint_story_mascot`, `paint_digit_prop`, `paint_whistle`.
- Produces:
  - `pub enum Layer { Back, Dark, Inside, Wrap(Option<Rect>), Front }`, the mockup's `compose` order. `Back` is clipped to "window minus island" (even-odd), painted before the island's fill; `Dark` is clipped to the island; then the clock; `Inside` unclipped; `Wrap(r)` clipped to the island (and to `r` when given); `Front` unclipped.
  - `pub struct Part { pub layer: Layer, pub draw: Draw, pub alpha: f32 }` and `Shot.parts: Vec<Part>`, replacing `Shot.props`.
  - `pub enum Draw` with exactly the primitives the stories need: `Fill { path: Vec<Seg>, rgb: u32 }`, `Stroke { path: Vec<Seg>, rgb: u32, width: f32 }`, `Ellipse { c: (f32, f32), r: (f32, f32), rgb: u32 }`, `Head { at: (f32, f32), shot: MascotShot }` (a mascot head anywhere, through the same `RobotPainter::paint` call as the slot head), `Digit { index, at, rot_deg }`, `Whistle { at }`, and `Group { at: (f32, f32), rot_deg: f32, scale: (f32, f32), pivot: (f32, f32), items: Vec<Draw> }` so a whole figure on the 64-grid moves in one transform. Paths are `crate::ui::robot::Seg` (the mascot's own segment type), in window dip or in the group's grid.
  - `Draw::bounds(&self) -> Rect`; `Shot::bounds()` is the union of the island and every part.
  - `Shot.mascot` (the slot head, painted in `Dark`) stays as it is.

- [ ] **Step 1: Write the failing tests**
  - In `stories/mod.rs`: `layers_paint_in_the_mockups_order` (the renderer's order list equals `[Back, island fill, Dark, clock, Inside, Wrap, Front]`, from one shared `const` the renderer iterates), and `a_group_moves_its_items_by_its_transform` (bounds of a `Group` with a known rotation and scale).
  - In `render.rs`, over the real `paint_island` (as part 1's `a_story_prop_is_clipped_to_the_island` does), both styles, solid and see-through fill:
    - `a_back_part_never_shows_inside_the_island`: a `Back` fill covering the whole window leaves zero ink of its colour inside the island outline (sample on a grid) and full ink outside it.
    - `a_dark_part_never_shows_outside_the_island`.
    - `a_wrap_part_is_cut_to_its_rect_and_the_island`.
    - `a_front_part_shows_everywhere`.
    - `a_head_part_carries_its_accessory`: a `Draw::Head` outside the island with an accessory set paints the accessory's colour inside its box.
  - Move `maling_angka`'s tests to the new fields unchanged in meaning.
- [ ] **Step 2: Run them to verify they fail** — `cargo test --locked ui::play::stories` and `cargo test --locked ui::render`: FAIL (`Layer`, `Part`, `Draw` not found).
- [ ] **Step 3: Implement.** `Back` uses a geometry group of the window rectangle and the played island geometry with `D2D1_FILL_MODE_ALTERNATE` as the layer's mask, built once per frame and only while a play runs. Paths become `ID2D1PathGeometry` per frame (a story has at most about 60). Migrate Maling angka: its digit and whistle become `Front` parts (see "Decision carried over"); its slot head stays `Shot.mascot`.
- [ ] **Step 4: Run the tests** — focused, then `cargo test --locked`: PASS (only the known `app::record` Z: failure).
- [ ] **Step 5: Commit** — `feat(play): the mockup's layer stack and drawables`

---

### Task 2: Part 1 leftovers

**Files:**
- Modify: `src/app.rs` (`input_ends_play` and its caller)

**Interfaces:**
- Consumes: `input_ends_play`, `advance_play`, `ActivePlay`.
- Produces: `KEY_GRACE: Duration = 250 ms`; keyboard input in the first `KEY_GRACE` of a play does not end it (mouse buttons and pointer-near still do).

- [ ] **Step 1: Write the failing test** `a_key_in_the_first_quarter_second_does_not_end_a_play` beside the existing `input_ends_play` cases: a key at 0.1 s keeps the play, a key at 0.3 s ends it, a mouse button at 0.1 s ends it.
- [ ] **Step 2: Run it** — `cargo test --locked input_ends`: FAIL.
- [ ] **Step 3: Implement**, and correct the inverted ordering comment beside the per-frame input reads (read the input tick before the cursor, and say why).
- [ ] **Step 4: Run** `cargo test --locked`: PASS.
- [ ] **Step 5: Commit** — `fix(play): the key that picks the menu item does not end the play`

---

### Task 3: Tubes and limbs

**Files:**
- Create: `src/ui/play/art/mod.rs`, `src/ui/play/art/tube.rs`
- Modify: `src/ui/play/mod.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks but `Seg`.
- Produces, ported from the mockup's helpers (lines ~265-370) as pure functions on `(f32, f32)` points:
  - `catmull_rom(pts: &[(f32, f32)], seg: usize) -> Vec<(f32, f32)>` (`cr`), `along(path, len, n) -> Vec<(f32, f32)>`, `curl(path, ang, len, m, sway) -> Vec<(f32, f32)>`, `ik(a, b, l1, l2, bend) -> (f32, f32)` (the knee/elbow), `normals(path) -> Vec<(f32, f32)>`.
  - `pub struct LimbStyle { w0, w1, wf: Option<fn(f32) -> f32>, pow, wm, fluff, cap, side_shade: Option<(i8, f32)>, stripes: Option<Stripes>, tip: Option<(f32, u32)>, suckers: Option<Suckers> }` and `limb(path, &LimbStyle, rgb) -> Vec<Draw>` (the tapered tube outline as one `Fill`, the side strip, stripes, the tip, suckers), the mockup's `limb`.

- [ ] **Step 1: Write the failing tests** in `tube.rs`: `catmull_rom_passes_through_its_points`; `along_returns_n_plus_one_points_spaced_evenly`; `a_tube_is_as_wide_as_its_width_function` (sample the outline's half-width at s = 0, 0.5, 1 against `w0`, the midpoint, `w1`, within 0.05); `ik_keeps_both_bone_lengths`; `a_limb_has_no_nan_for_a_degenerate_path` (two equal points, one point).
- [ ] **Step 2: Run** `cargo test --locked ui::play::art`: FAIL.
- [ ] **Step 3: Implement** by porting the mockup's functions line for line; allocate per call (only while a play runs).
- [ ] **Step 4: Run** `cargo test --locked`: PASS.
- [ ] **Step 5: Commit** — `feat(play): tubes and limbs for the new mascot art`

---

### Task 4: The seated cat's body

**Files:**
- Create: `src/ui/play/art/cat.rs`
- Modify: `src/ui/play/art/mod.rs`, `src/ui/demo.rs` (one static scene)

**Interfaces:**
- Consumes: `limb`, `Draw`, `CatStyle` (head colour, inks, rim), the head as `Draw::Head`.
- Produces: `pub struct CatPose { dk, rise, head: (f32, f32, f32) /* hx, hy, hr */, face: MascotShot, paws: [Paw; 2], tail: Option<Tail> }` (the mockup's `catSlim` options) and `cat(pose: &CatPose, look: CatLook) -> Draw` (a `Group` on the 64-grid, seat at grid (32, 86), `CAT_SC` 0.94): the tail behind, body, side shade, two haunches, two front legs (limbs), the chin shadow, the head (`Draw::Head` at the neck transform), the paws in front, toe beans when a paw is lifted.
- Colours: the mockup's `CS` table (fur, body, haunch, side, shade, bean) mapped to each coat by its relation to the mockup's terracotta (body = head colour, the darker tones by the same `shade`/mix the head uses). Calico: white body with one orange and one dark patch, cut to the body path as the head's patches are; Black: the head's rim colour as a 0.8-unit outline round the silhouette.

- [ ] **Step 1: Write the failing tests** in `cat.rs`: `the_body_takes_every_coats_colours` (the four coats give four different body fills, each equal to that coat's head colour); `calico_has_two_patches_on_the_body`; `black_has_a_rim_round_the_body`; `a_lifted_paw_shows_its_beans` (lift 0.5 adds the four bean ellipses, lift 0.4 does not); `the_head_sits_on_the_neck_whatever_the_rise` (the `Draw::Head` anchor follows `rise` by `14 * (1 - rise)` as the mockup does); `the_tail_is_dropped_when_crouched` (dk ≥ 0.6 → no tail).
- [ ] **Step 2: Run** `cargo test --locked ui::play::art::cat`: FAIL.
- [ ] **Step 3: Implement**; add the demo scene `bata-play-cat-body` (a seated cat beside the island, tail up, one paw lifted) with `DI_CAT_COLOR` honoured.
- [ ] **Step 4: Run** `cargo test --locked`: PASS; `cargo build --locked`.
- [ ] **Step 5: Commit** — `feat(play): the seated cat's body and tail`

The controller captures `bata-play-cat-body` in all four coats and with an accessory, for the owner, before Task 7 starts.

---

### Task 5: The octopus's long arms

**Files:**
- Create: `src/ui/play/art/octopus.rs`
- Modify: `src/ui/play/art/mod.rs`, `src/ui/demo.rs`

**Interfaces:**
- Consumes: `limb` (with suckers), `Draw`, `octopus_rgb`, `shade`, `sucker_rgb`, the head as `Draw::Head`.
- Produces: `arm(path: &[(f32, f32)], coat: OctopusColor, side: i8, wm: f32) -> Vec<Draw>` (the mockup's `octArm`: w0 4.3, w1 0.9, pow 0.75, side shade 0.34, suckers every 2.3 from 1.5) and `root(mantle_at, i, rot, sx, sy) -> (f32, f32)` (the mockup's `OROOT`/`rootAt`), so a story builds each arm's centre line from `rim` and attaches it to the mantle.

- [ ] **Step 1: Write the failing tests**: `an_arm_tapers_from_root_to_tip`; `suckers_sit_on_the_inner_side`; `every_coat_colours_the_arm_like_the_head`; `the_roots_follow_the_mantle_turn`.
- [ ] **Step 2: Run** `cargo test --locked ui::play::art::octopus`: FAIL.
- [ ] **Step 3: Implement**; demo scene `bata-play-octopus-arms` (the head in the slot, two arms wrapping the right end).
- [ ] **Step 4: Run** `cargo test --locked`: PASS; `cargo build --locked`.
- [ ] **Step 5: Commit** — `feat(play): the octopus's long arms`

---

### Task 6: The falk's wings and body

**Files:**
- Create: `src/ui/play/art/falk.rs`
- Modify: `src/ui/play/art/mod.rs`, `src/ui/demo.rs`

**Interfaces:**
- Consumes: `limb`, `Draw`, `falk_ink`, `mascot::feather` (the existing feather capsule), the head as `Draw::Head`.
- Produces: `pub struct WingPose { arm, hand, spread, fold }` with `WingPose::mix(a, b, t)` (the mockup's `wmix`), the named poses the mockup uses (folded, open, hang, flap-up, flap-down), `wing(side: i8, shoulder: (f32, f32), now: WingPose, before: WingPose, coat) -> Vec<Draw>` (base shape, 5 primaries from `before` so the tip lags, 4 secondaries, one covert row, the bone as a tube), and `bird(pose: &BirdPose, look) -> Draw` (body, tail plume, legs, both wings, the head), the mockup's `bird`.
- The lag is arithmetic: a story evaluates its pose at `t - 0.07` for `before`; no timer.

- [ ] **Step 1: Write the failing tests**: `a_folded_wing_is_inside_the_body_outline`; `an_open_wing_has_ten_feathers_and_a_covert_row`; `the_primaries_use_the_pose_before`; `every_coat_colours_the_wing_like_the_head`; `pose_mix_at_0_and_1_returns_the_ends`.
- [ ] **Step 2: Run** `cargo test --locked ui::play::art::falk`: FAIL.
- [ ] **Step 3: Implement**; demo scenes `bata-play-falk-open` and `bata-play-falk-hang`.
- [ ] **Step 4: Run** `cargo test --locked`: PASS; `cargo build --locked`.
- [ ] **Step 5: Commit** — `feat(play): the falk's wings and body`

---

## The story tasks (7-15)

Every story task has the same shape; only the specifics below differ.

**Files:** Create `src/ui/play/stories/<module>.rs`; modify `src/ui/play/stories/mod.rs` (`pub mod`, the `shot` match arm, `ready` gains the story) and `src/ui/demo.rs` (the scenes; update the `SCENES` count).

**Interfaces:** `pub fn shot(style: Style, s: ShapeRect, beat: Beat) -> Shot` in the module, reached through `stories::shot`.

- [ ] **Step 1: Write the failing tests.** The four contract tests of part 1's Task 13 (`the_story_starts_and_ends_at_rest_in_both_styles`, `alpha_zero_means_no_deformation_at_any_moment`, `nothing_leaves_the_window_at_any_size_or_style`, and the rest-near-the-ends test for the mascot) through one shared macro `story_contract_tests!(Story::X, shot)`, which Task 7 adds to `stories/mod.rs` and moves Maling angka onto; plus the story's own tests listed below.
- [ ] **Step 2: Run** `cargo test --locked ui::play::stories::<module>`: FAIL.
- [ ] **Step 3: Implement the timeline** by porting the mockup story's `draw(t, L)`: each `L.back`/`dark`/`inside`/`wrap`/`front` string becomes `Part`s of that `Layer`, each `L.notch` value a field of `Deform`, each `L.clock[i]` a `DigitFx`. Scale every amplitude by `beat.alpha` toward rest, as Maling angka does; settle rings to exactly zero before the end if the rest test needs it.
- [ ] **Step 4: Run** `cargo test --locked`: PASS; `rm -rf target/test-tmp`; `cargo build --locked`.
- [ ] **Step 5: Commit** — `feat(play): <story in English>`

The controller then captures the demo scenes in both styles at 160x28, 190x34 and 300x44, solid and see-through, compares them with the mockup's sheet at the same times, and fixes any visual finding in the task's fix loop before the next story.

### Task 7: Cat — Syal ekor (`c4`, 8.4 s, `syal_ekor.rs`)
- The island: the left end squeezed by the tail (`Deform.squeeze` toward the left end), a sneeze as a short x shift.
- Layers: the tail behind the left end (`Back`), its end lying on the island's face (`Wrap`, clipped to the island), the cat beside the left end (`Back`/`Front` as the mockup's layers say).
- Own tests: `the_tail_follows_the_rim_round_the_left_end` (its centre line points lie within 1 dip of `rim` at the mockup's gap); `the_sneeze_shifts_the_island_and_returns`.
- Demo: `bata-play-syal-wrap`, `bata-play-syal-sneeze`.

### Task 8: Cat — Petak umpet (`c1`, 7.8 s, `petak_umpet.rs`)
- The island: a bump runs along the lip (a moving `Dent`), it rocks on the pounce (`swing_deg` about the edge on the Notch; on the Pill about its centre plus a small spring in y).
- Layers: the peeking head clipped "outside the island" (`Back`), the small paws gripping the edge after the island (`Front`).
- Own tests: `the_bump_runs_left_to_right_along_the_lip`; `the_notch_rocks_about_the_screen_edge_and_the_pill_about_its_centre`; `the_peeking_head_is_never_inside_the_island`.
- Demo: `bata-play-petak-peek`, `bata-play-petak-pounce`.
- Watch item: part 1's Notch shoulder tuck is 3 dip; check the swing leaves no ledge.

### Task 9: Cat — Bantal angka (`c2`, 8.6 s, `bantal_angka.rs`, needs the clock)
- The island: the lip rises and falls with the breathing (a broad dent whose depth breathes); the minute swaps 1 → 2 with a little overshoot.
- Needs `DigitFx` to show a different character: add `DigitFx.ch: Option<char>` (the minute's crossing texts), drawn through the same per-character path. The swap uses the clock's real next minute, not the mockup's literal "2".
- Own tests: `the_lip_breathes_and_settles`; `the_minute_swap_crosses_two_texts`.
- Demo: `bata-play-bantal-sleep`, `bata-play-bantal-swap`.

### Task 10: Cat — Piano ekor (`c3`, 8.4 s, `piano_ekor.rs`, needs the clock)
- The island: the digits drop one by one with a small glow under each (`Draw::Ellipse`, translucent), a light sweep at the end that bends round the right end's arc (radius 16 on the Notch, h/2 on the Pill).
- The tail's arc height is tied to the island's top edge, so on the Pill it stays inside.
- Own tests: `each_digit_drops_once_in_order`; `the_sweep_follows_the_right_ends_radius`.
- Demo: `bata-play-piano-tap`, `bata-play-piano-sweep`.

### Task 11: Octopus — Penasaran jam (`o1`, 7.6 s, `penasaran_jam.rs`, needs the clock)
- The island: none; the "1" is pulled off, wobbles and is put back (a `Draw::Digit` in `Front` while carried, the clock's own character hidden meanwhile, as Maling angka does).
- The arm is drawn twice: behind (`Back`) and the part on the island's face (`Wrap`). The "plop" ring is a growing, fading `Stroke` circle.
- Own tests: `the_pulled_digit_is_drawn_once_at_any_moment`; `the_arm_wraps_the_right_end_on_both_styles`.
- Demo: `bata-play-penasaran-pull`, `bata-play-penasaran-plop`.

### Task 12: Octopus — Peluk pulau (`o2`, 7.8 s, `peluk_pulau.rs`)
- The island: squeezed (x 0.97, y 1.08) then springs back (the Notch from the screen edge, the Pill from its centre).
- Four long arms of about 40 points each; the tips at `rim` u = 0.25 (the Notch's straight side, the Pill's top edge).
- Own tests: `the_hug_squeezes_and_springs_back_to_rest`; `four_arms_and_their_tips_on_the_rim`.
- Demo: `bata-play-peluk-hug`.

### Task 13: Octopus — Bungee (`o3`, 7.4 s, `bungee.rs`)
- The island: the Pill shifts down by rope tension x 1.4 dip and springs back; the clock follows (it already does through the island's transform); the Notch does nothing.
- The holding arm thins as it stretches (width divided by the square root of its length).
- Own tests: `only_the_pill_is_pulled_down`; `the_rope_thins_as_it_stretches`.
- Demo: `bata-play-bungee-drop`, `bata-play-bungee-bounce`.

### Task 14: Falk — Kelelawar (`f1`, 7.6 s, `kelelawar.rs`)
- The island: the Pill tilts with the bird's swing x 0.06 deg about its centre; the Notch does nothing. The grip is on the straight lip, the same on both styles.
- Uses `bird` with the hang and open wing poses; the primaries lag by 70 ms.
- Own tests: `only_the_pill_tilts_with_the_swing`; `the_feet_stay_on_the_lip`.
- Demo: `bata-play-kelelawar-hang`, `bata-play-kelelawar-open`.

### Task 15: Falk — Terbang keliling (`f3`, 7.9 s, `terbang_keliling.rs`)
- The island: nothing. One Catmull-Rom flight path from the island's edges (slot, right end, lip; `f3path` in the mockup), the tilt from the path's direction, a 360-degree roll on one stretch, flapping (arm angle up and down, the fan closing on the up-stroke); out of and back into the slot by folding the body (y scale) under the head, clipped "outside the island"; a falling feather by opacity. Behind the right end the points sit inside its arc so the bird truly disappears on the Pill.
- Own tests: `the_bird_is_hidden_behind_the_right_end`; `the_path_holds_at_160_and_300_wide`.
- Also, now that all ten stories are ready: `with_the_clock_off_every_mascot_keeps_two_stories` over the real `pool`, and `every_story_is_ready` in `stories/mod.rs`.
- Demo: `bata-play-terbang-loop`, `bata-play-terbang-roll`.

---

## After this plan

Finishing: merge locally into `main`, `cargo test --locked`, update `docs/memory/mascot-play.md` (part 2 done, the layer stack, the art modules) and `README.md` (the cat and octopus stories, no longer "coming"), push `main`, clean up the worktree. The Settings row's "Animasi untuk maskot ini menyusul." line then never shows; leave the code (it guards a future mascot) unless the final review says otherwise.
