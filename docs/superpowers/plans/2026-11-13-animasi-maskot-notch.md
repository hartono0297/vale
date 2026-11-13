# Mascot Island Animations — Implementation Plan (part 1: the engine)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the engine that plays a mascot animation around the closed island, in both styles, and ship one story end to end (the falk's "Maling angka").

**Architecture:** A new `src/ui/play/` module holds pure logic: a rim function that maps a fraction of the island's lower outline to a point and a normal for both styles, an outline deformer, the trigger rules, and a play state machine. The renderer gains a deformed-island path, an inverse clip for everything drawn behind the island, and a per-character clock used only while a play runs. `app.rs` gains one one-shot timer and the gates that arm it, plus a tray item that plays one on demand.

**Tech Stack:** Rust edition 2024, Win32 + Direct2D (`windows` crate), no new dependencies. Tests are `#[cfg(test)]` modules inside `src/**`, run with `cargo test --locked`.

**Spec:** `docs/superpowers/specs/2026-11-13-animasi-maskot-notch-design.md`

**Reference drawing:** `docs/design/maskot-notch-v3.html` — round 8. Its JS holds every coordinate, timing curve and deformation amount, derived from the real geometry constants, for both styles and all sizes. Read it for any number this plan does not pin.

## Global Constraints

- Windows 11 x64 only; stable MSVC toolchain; `cargo build --release --locked`.
- Island sizes: width 160–300 dip (`NOTCH_W_MIN`/`NOTCH_W_MAX`, default 190), height 28–44. Every timeline is a fraction of the island's width and height, never a fixed dip value.
- Styles: `Style::Notch` (flush with the screen's top edge, bottom corner radius `s.h - shoulder_depth(s)`, `SHOULDER` = 12) and `Style::Pill` (floats `PILL_TOP` = 8.0 dip below the top edge, corner radius `s.h / 2`).
- Idle layout at rest: mascot `ROBOT_COMPACT` 26 + `ROBOT_GAP` 8 + `CLOCK_W` 44 = 78 dip, centred (`geometry::idle_layout_lead`).
- No new repeating timer, no new thread, no polling, no new `WM_APP`. One new one-shot timer id only.
- Nothing new runs while no animation runs: an idle island costs exactly what it costs today.
- All motion is off when `Motion` is not `Full`, when Windows' Energy Saver is on, or when Windows animations are off; in those states a play never starts and a running one ends instantly with one draw.
- UI strings are Indonesian with an English counterpart in `i18n`, matching the existing rows.
- Nothing may rely on the island being opaque to hide a part: paint order is (1) behind the island clipped to "screen minus island", (2) inside-the-dark parts clipped to the island shape, (3) the island's fill and bezel.

## Review Focus

- **The island's size or style changes while a play runs** (Settings sliders, `adopt_config`). A play holding dip values from the old shape would tear. Expected: the play winds down at once. Test in Task 9.
- **Energy Saver or Windows' animation setting flips mid-play** (`check_battery`, `WM_SETTINGCHANGE`). Expected: the play ends instantly with one draw and the island returns to its exact resting outline. Test in Task 6.
- **The system clock jumps mid-play** (`WM_TIMECHANGE`, resume) during a story that holds a digit. Expected: the play winds down rather than animating a digit that no longer exists. Test in Task 6.
- **`idle_clock` is switched off while a clock story runs.** Expected: the play winds down (Task 6's `must_stop`); the story also leaves the pool for the next pick (Task 4's `pool`).
- **The pointer enters the island's hover zone one frame after a play starts.** Expected: the play winds down and the panel opens as usual; the island is never left deformed. Test in Task 6.

---

### Task 1: The rim function

A point and a normal anywhere along the island's lower outline, for both styles, so every limb that winds round an end has one path instead of two tables of points.

**Files:**
- Create: `src/ui/play/mod.rs` (declares the submodules), `src/ui/play/rim.rs`
- Modify: `src/ui/mod.rs` (add `pub mod play;`)

**Interfaces:**
- Consumes: `geometry::{ShapeRect, shoulder_depth, corner_radius, shape_contains}`, `config::Style`.
- Produces: `pub struct RimPt { pub x: f32, pub y: f32, pub nx: f32, pub ny: f32, pub tx: f32, pub ty: f32 }` and `pub fn rim(style: Style, s: ShapeRect, u: f32) -> RimPt`.

`u` runs 0.0 to 1.0 along the island's **lower** outline, left to right: from the top of the left edge, down it, round the bottom-left end, along the lip, round the bottom-right end, up to the top of the right edge. On the Notch "the top" is the screen edge (`s.y`); on the Pill it is the top of the half-circle end. `n` is the outward unit normal, `t` the unit tangent in the direction of increasing `u`. `u` outside [0, 1] is clamped.

- [ ] **Step 1: Write the failing tests** in `src/ui/play/rim.rs`

```rust
#[test]
fn rim_midpoint_is_the_lip_centre_in_both_styles() {
    for style in [Style::Notch, Style::Pill] {
        let s = shape(style, 190.0, 34.0);
        let p = rim(style, s, 0.5);
        assert!((p.x - (s.x + s.w / 2.0)).abs() < 0.01, "{style:?}");
        assert!((p.y - (s.y + s.h)).abs() < 0.01, "{style:?}");
        assert!((p.ny - 1.0).abs() < 0.01, "{style:?} normal points down");
    }
}

#[test]
fn rim_normals_point_out_of_the_shape() {
    for style in [Style::Notch, Style::Pill] {
        for i in 0..=100 {
            let u = i as f32 / 100.0;
            let s = shape(style, 190.0, 34.0);
            let p = rim(style, s, u);
            assert!(
                !shape_contains(style, s, p.x + p.nx * 2.0, p.y + p.ny * 2.0),
                "{style:?} u={u}"
            );
        }
    }
}

#[test]
fn rim_is_continuous_at_every_size() {
    for style in [Style::Notch, Style::Pill] {
        for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
            let s = shape(style, w, h);
            let mut prev = rim(style, s, 0.0);
            for i in 1..=1000 {
                let p = rim(style, s, i as f32 / 1000.0);
                let step = ((p.x - prev.x).powi(2) + (p.y - prev.y).powi(2)).sqrt();
                assert!(step < 2.0, "{style:?} {w}x{h} jumped {step} at {i}");
                prev = p;
            }
        }
    }
}

#[test]
fn the_pill_ends_are_half_circles_and_the_notch_sides_are_straight() {
    let pill = shape(Style::Pill, 190.0, 34.0);
    let left = (0..=200).map(|i| rim(Style::Pill, pill, i as f32 / 2000.0))
        .min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    assert!((left.y - (pill.y + pill.h / 2.0)).abs() < 0.3, "leftmost point is at the end's centre height");

    let notch = shape(Style::Notch, 190.0, 34.0);
    let top = rim(Style::Notch, notch, 0.0);
    assert!((top.y - notch.y).abs() < 0.01, "the notch's side starts at the screen edge");
    assert!((top.x - notch.x).abs() < 0.01);
}
```

Two shared test helpers live in `src/ui/play/mod.rs` in a `#[cfg(test)] pub(crate) mod testing` module, because Tasks 2, 5, 6, 10, 11 and 13 all reach for them across module boundaries:

- `shape(style: Style, w: f32, h: f32) -> ShapeRect` — the resting island at that size, with `corner_radius` applied, `y = 0.0` for the Notch and `y = PILL_TOP` for the Pill. Created here, in Task 1.
- `ok_env() -> Env` — every gate satisfied, `clock_on: true`. Created in Task 6, when `Env` exists.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked ui::play::rim`
Expected: FAIL, `rim` not found.

- [ ] **Step 3: Implement `rim` in `src/ui/play/rim.rs`**

Split `u` by arc length over the segments of each style (Notch: left side, bottom-left arc, lip, bottom-right arc, right side; Pill: left half-circle, lip, right half-circle), so a given `u` lands the same proportional distance along the outline at any size. Mirror `src/ui/render.rs:notch_path` for which radius each corner uses.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked ui::play::rim`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/play/mod.rs src/ui/play/rim.rs src/ui/mod.rs
git commit -m "feat(play): a rim point and normal anywhere round the island"
```

---

### Task 2: The deformed outline

The island's outline as a point list that stories can dent, bulge, squeeze, swing, shift and tilt, with the pivot each style needs.

**Files:**
- Create: `src/ui/play/shape.rs`
- Modify: `src/ui/play/mod.rs`

**Interfaces:**
- Consumes: Task 1's `rim`.
- Produces:

```rust
pub const OUTLINE_PTS: usize = 90;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dent { pub u: f32, pub depth: f32, pub width: f32 }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deform {
    pub dents: Vec<Dent>,
    pub squeeze: f32,
    pub swing_deg: f32,
    pub shift: (f32, f32),
    pub tilt_deg: f32,
}

pub fn outline(style: Style, s: ShapeRect, d: &Deform) -> Vec<(f32, f32)>;
pub fn is_rest(d: &Deform) -> bool;
```

A `Dent` pushes the outline outward along its normal by `depth` (negative dents inward) over a bell of half-width `width`, both as fractions of `s.h`. `squeeze` scales the island in x about its centre. `swing_deg` rotates it; `shift` and `tilt_deg` are **Pill only** and are asserted zero for the Notch.

- [ ] **Step 1: Write the failing tests** in `src/ui/play/shape.rs`

```rust
#[test]
fn a_rest_deform_is_exactly_the_rim() {
    for style in [Style::Notch, Style::Pill] {
        let s = shape(style, 190.0, 34.0);
        let pts = outline(style, s, &Deform::default());
        assert_eq!(pts.len(), OUTLINE_PTS);
        for (i, (x, y)) in pts.iter().enumerate() {
            let p = rim(style, s, i as f32 / (OUTLINE_PTS - 1) as f32);
            assert!((x - p.x).abs() < 0.01 && (y - p.y).abs() < 0.01, "{style:?} point {i}");
        }
    }
}

#[test]
fn a_dent_moves_only_the_outline_near_it() {
    let s = shape(Style::Notch, 190.0, 34.0);
    let rest = outline(Style::Notch, s, &Deform::default());
    let d = Deform { dents: vec![Dent { u: 0.5, depth: -0.3, width: 0.08 }], ..Default::default() };
    let bent = outline(Style::Notch, s, &d);
    let far = rim(Style::Notch, s, 0.1);
    assert!(bent.iter().any(|p| (p.1 - (s.y + s.h)).abs() > 3.0), "the lip moved");
    assert!(
        bent.iter().zip(&rest).filter(|(b, r)| (b.0 - r.0).abs() + (b.1 - r.1).abs() > 0.01)
            .all(|(b, _)| (b.0 - far.x).abs() > 10.0),
        "nothing moved far from the dent"
    );
}

#[test]
fn a_swinging_notch_leaves_no_hole_at_the_screen_edge() {
    let s = shape(Style::Notch, 190.0, 34.0);
    let pts = outline(Style::Notch, s, &Deform { swing_deg: 6.0, ..Default::default() });
    assert!(pts.iter().any(|p| p.1 < s.y - 0.5), "the top is extended behind the screen edge");
    let crossings: Vec<f32> = pts.windows(2)
        .filter(|w| (w[0].1 - s.y) * (w[1].1 - s.y) <= 0.0)
        .map(|w| w[0].0)
        .collect();
    assert!(crossings.iter().any(|&x| x <= s.x), "still covers the left end at the edge");
    assert!(crossings.iter().any(|&x| x >= s.x + s.w), "and the right end");
}

#[test]
fn shift_and_tilt_are_pill_only() {
    let s = shape(Style::Notch, 190.0, 34.0);
    let d = Deform { shift: (0.0, 6.0), tilt_deg: 4.0, ..Default::default() };
    assert_eq!(outline(Style::Notch, s, &d), outline(Style::Notch, s, &Deform::default()));
}

#[test]
fn is_rest_is_true_only_for_an_untouched_deform() {
    assert!(is_rest(&Deform::default()));
    assert!(!is_rest(&Deform { squeeze: 0.02, ..Default::default() }));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked ui::play::shape`
Expected: FAIL, `outline` not found.

- [ ] **Step 3: Implement `outline`, `is_rest` and the types in `src/ui/play/shape.rs`**

Sample `rim` at `OUTLINE_PTS` evenly spaced `u`, apply each dent along the normal, then apply squeeze, swing, shift and tilt as one transform: about `(s.x + s.w / 2.0, s.y)` for the Notch, about the island's centre for the Pill, with `shift` and `tilt_deg` forced to zero on the Notch.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked ui::play::shape`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/play/shape.rs src/ui/play/mod.rs
git commit -m "feat(play): deform the island outline, with a pivot per style"
```

---

### Task 3: The `mascot_play` config key

**Files:**
- Modify: `src/config.rs`, `src/config_transfer.rs` (or wherever `settings-transfer`'s allowlist lives — find it with `rg "fn classify|allowlist" src/`)

**Interfaces:**
- Produces: `pub enum MascotPlay { Sering, Sesekali, Jarang, Sapaan, Mati }` with `ALL`, and `Config::mascot_play: MascotPlay`, default `Sesekali`, placed at the end of `Config` and read leniently.

- [ ] **Step 1: Write the failing tests** in `src/config.rs`'s test module

```rust
#[test]
fn mascot_play_defaults_to_sesekali_and_reads_leniently() {
    assert_eq!(Config::default().mascot_play, MascotPlay::Sesekali);
    assert_eq!(parse(r#"{"mascot_play":"jarang"}"#).mascot_play, MascotPlay::Jarang);
    assert_eq!(parse(r#"{"mascot_play":"kadang2"}"#).mascot_play, MascotPlay::Sesekali);
    assert_eq!(parse(r#"{"mascot_play":7}"#).mascot_play, MascotPlay::Sesekali);
    let odd = parse(r#"{"mascot_play":null,"robot_sleep_minutes":25}"#);
    assert_eq!((odd.mascot_play, odd.robot_sleep_minutes), (MascotPlay::Sesekali, 25));
}
```

The existing "every config key is classified" test from [[settings-transfer]] will now fail until the new key is classified; classify `mascot_play` as a general setting that transfers between PCs.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked config`
Expected: FAIL on the new test and on the classification test.

- [ ] **Step 3: Add `MascotPlay`, the field, its lenient deserializer and its classification**

Follow `robot_sleep_minutes` and `mascot` for the lenient pattern: a bad value must never fail the rest of the file ([[config-parse-default-fallback]]).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked config`
Expected: PASS, including the classification test.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/config_transfer.rs
git commit -m "feat(config): mascot_play, how often the mascot plays"
```

---

### Task 4: The trigger rules

**Files:**
- Create: `src/ui/play/trigger.rs`
- Modify: `src/ui/play/mod.rs`

**Interfaces:**
- Consumes: `config::MascotPlay`, `theme::Mascot`.
- Produces:

```rust
pub const PRESENCE_MIN: Duration = Duration::from_secs(15);
pub const PRESENCE_MAX: Duration = Duration::from_secs(300);
pub const WIND_DOWN: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment { Idle, Hour, Greeting, Manual }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Story { PetakUmpet, BantalAngka, PianoEkor, SyalEkor,
                 PenasaranJam, PelukPulau, Bungee,
                 Kelelawar, MalingAngka, TerbangKeliling }

impl Story {
    pub fn mascot(self) -> Mascot;
    pub fn needs_clock(self) -> bool;
    pub fn secs(self) -> f32;
}

pub fn presence_ok(since_input: Duration) -> bool;
pub fn pool(mascot: Mascot, clock_on: bool) -> Vec<Story>;
pub fn pick(mascot: Mascot, clock_on: bool, last: Option<Story>, moment: Moment, roll: u32) -> Story;
pub fn wait_for(preset: MascotPlay, roll: u32) -> Option<Duration>;
pub fn cooldown(preset: MascotPlay) -> Duration;
```

Values, copied from the spec: Sering 8–15 min wait / 6 min cooldown; Sesekali 15–35 / 12; Jarang 45–90 / 40; Sapaan no wait / 12; Mati no wait and no automatic play. `Moment::Hour` prefers a story with `needs_clock()`; `Moment::Greeting` prefers SyalEkor, PelukPulau or TerbangKeliling for its mascot.

- [ ] **Step 1: Write the failing tests** in `src/ui/play/trigger.rs`

```rust
#[test]
fn the_presence_window_is_fifteen_seconds_to_five_minutes() {
    assert!(!presence_ok(Duration::from_secs(14)));
    assert!(presence_ok(Duration::from_secs(15)));
    assert!(presence_ok(Duration::from_secs(299)));
    assert!(!presence_ok(Duration::from_secs(301)));
}

#[test]
fn every_mascot_keeps_at_least_two_stories_without_the_clock() {
    for m in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
        assert!(pool(m, true).len() >= 3, "{m:?}");
        assert!(pool(m, false).len() >= 2, "{m:?} without the clock");
        assert!(pool(m, false).iter().all(|s| !s.needs_clock()), "{m:?}");
        assert!(pool(m, true).iter().all(|s| s.mascot() == m), "{m:?}");
    }
}

#[test]
fn a_pick_never_repeats_the_last_story() {
    for m in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
        for roll in 0..50 {
            let last = pool(m, true)[0];
            assert_ne!(pick(m, true, Some(last), Moment::Idle, roll), last, "{m:?} roll={roll}");
        }
    }
}

#[test]
fn the_hour_prefers_a_clock_story_and_the_greeting_a_warm_one() {
    for roll in 0..20 {
        assert!(pick(Mascot::Cat, true, None, Moment::Hour, roll).needs_clock());
        assert_eq!(pick(Mascot::Octopus, true, None, Moment::Greeting, roll), Story::PelukPulau);
    }
    assert!(!pick(Mascot::Cat, false, None, Moment::Hour, 0).needs_clock(), "no clock, no clock story");
}

#[test]
fn the_presets_carry_the_specs_waits_and_cooldowns() {
    assert_eq!(wait_for(MascotPlay::Mati, 0), None);
    assert_eq!(wait_for(MascotPlay::Sapaan, 0), None);
    for roll in 0..100 {
        let w = wait_for(MascotPlay::Sesekali, roll).unwrap().as_secs();
        assert!((15 * 60..=35 * 60).contains(&w), "{w}");
    }
    assert_eq!(cooldown(MascotPlay::Sering), Duration::from_secs(6 * 60));
    assert_eq!(cooldown(MascotPlay::Jarang), Duration::from_secs(40 * 60));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked ui::play::trigger`
Expected: FAIL, the module's items are not defined.

- [ ] **Step 3: Implement the module**

`roll` is a caller-supplied random `u32` so every function stays pure and testable; the app passes one from the same source the blink's random wait uses.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked ui::play::trigger`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/play/trigger.rs src/ui/play/mod.rs
git commit -m "feat(play): when a story plays and which one"
```

---

### Task 5: The play state machine and the "island makes room" beat

**Files:**
- Create: `src/ui/play/state.rs`
- Modify: `src/ui/play/mod.rs`

**Interfaces:**
- Consumes: Task 2's `Deform`, Task 4's `Story`, `WIND_DOWN`.
- Produces:

```rust
pub struct Play { /* story, started, ending: Option<Instant> */ }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beat { pub spread: f32, pub alpha: f32, pub t: f32 }

impl Play {
    pub fn start(story: Story, now: Instant) -> Self;
    pub fn story(&self) -> Story;
    pub fn beat(&self, now: Instant) -> Beat;
    pub fn end(&mut self, now: Instant);
    pub fn done(&self, now: Instant) -> bool;
    pub fn finish_now(&mut self);
}

pub fn spread_offsets(layout: IdleLayout, s: ShapeRect, spread: f32) -> IdleLayout;
```

`Beat::t` is the story's own time in seconds, `spread` eases 0 to 1 over the first `WIND_DOWN` and back to 0 over the last, and `alpha` scales every deformation so a wind-down returns the island to rest. `spread_offsets` eases the mascot to the island's left end and the clock to its right end.

- [ ] **Step 1: Write the failing tests** in `src/ui/play/state.rs`

```rust
#[test]
fn a_wind_down_takes_two_hundred_ms_and_lands_at_rest() {
    let t0 = Instant::now();
    let mut p = Play::start(Story::MalingAngka, t0);
    p.end(t0 + Duration::from_secs(1));
    assert!(!p.done(t0 + Duration::from_millis(1199)));
    assert!(p.done(t0 + Duration::from_millis(1200)));
    let last = p.beat(t0 + Duration::from_millis(1200));
    assert_eq!((last.alpha, last.spread), (0.0, 0.0), "the island is exactly at rest");
}

#[test]
fn ending_twice_does_not_extend_the_wind_down() {
    let t0 = Instant::now();
    let mut p = Play::start(Story::Bungee, t0);
    p.end(t0 + Duration::from_secs(1));
    p.end(t0 + Duration::from_millis(1100));
    assert!(p.done(t0 + Duration::from_millis(1200)));
}

#[test]
fn finish_now_lands_at_rest_in_one_step() {
    let t0 = Instant::now();
    let mut p = Play::start(Story::Kelelawar, t0);
    p.finish_now();
    assert!(p.done(t0 + Duration::from_millis(1)));
    assert_eq!(p.beat(t0 + Duration::from_millis(1)).alpha, 0.0);
}

#[test]
fn a_story_winds_down_by_itself_at_the_end_of_its_time() {
    let t0 = Instant::now();
    let p = Play::start(Story::MalingAngka, t0);
    let secs = Story::MalingAngka.secs();
    assert!(!p.done(t0 + Duration::from_secs_f32(secs - 0.1)));
    assert!(p.done(t0 + Duration::from_secs_f32(secs + 0.3)));
}

#[test]
fn spread_moves_the_mascot_left_and_the_clock_right() {
    let s = shape(Style::Notch, 190.0, 34.0);
    let rest = idle_layout_lead(s, true, true, 0.0);
    let out = spread_offsets(rest, s, 1.0);
    assert!(out.robot.unwrap().x < rest.robot.unwrap().x - 20.0);
    assert!(out.clock.unwrap().x > rest.clock.unwrap().x + 20.0);
    assert_eq!(spread_offsets(rest, s, 0.0), rest, "no spread, no move");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked ui::play::state`
Expected: FAIL, `Play` not found.

- [ ] **Step 3: Implement `Play`, `Beat` and `spread_offsets`**

`finish_now` is the instant path for Motion not Full, Energy Saver and Windows animations off.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked ui::play::state`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/play/state.rs src/ui/play/mod.rs
git commit -m "feat(play): the play clock, the wind-down and the spread beat"
```

---

### Task 6: Gates — what stops a play

**Files:**
- Create: `src/ui/play/gate.rs`
- Modify: `src/ui/play/mod.rs`

**Interfaces:**
- Consumes: Task 4's `presence_ok`, `Moment`, `MascotPlay`.
- Produces:

```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct Env {
    pub mascot_on: bool, pub motion_full: bool, pub energy_saver: bool, pub win_anim: bool,
    pub idle_view: bool, pub panel_open: bool, pub menu_open: bool,
    pub fullscreen: bool, pub screen_away: bool, pub shrunk: bool,
    pub pointer_near: bool, pub asleep: bool, pub recording: bool, pub clock_on: bool,
}

pub fn may_start(env: &Env, moment: Moment, preset: MascotPlay, since_input: Duration, since_last: Duration) -> bool;
pub fn must_stop(env: &Env, playing: Story, shape_changed: bool, clock_jumped: bool) -> bool;
pub fn instant_stop(env: &Env) -> bool;
```

`Moment::Manual` skips the presence rule, the cooldown and `idle_view`'s panel clause, but not `mascot_on`, `motion_full`, `fullscreen`, `screen_away` or `recording`.

- [ ] **Step 1: Write the failing tests** in `src/ui/play/gate.rs`

```rust
#[test]
fn every_gate_can_stop_an_automatic_play() {
    let base = (ok_env(), Moment::Idle, MascotPlay::Sesekali, Duration::from_secs(60), Duration::from_secs(3600));
    assert!(may_start(&base.0, base.1, base.2, base.3, base.4));
    // One entry per Env field EXCEPT clock_on, which changes which story is
    // picked (Task 4's `pool`) and never the permission to start.
    for (name, env) in off_by_one_envs() {
        assert!(!may_start(&env, base.1, base.2, base.3, base.4), "{name} did not stop it");
    }
}

#[test]
fn the_clock_being_off_does_not_stop_a_play_from_starting() {
    let mut env = ok_env();
    env.clock_on = false;
    assert!(may_start(&env, Moment::Idle, MascotPlay::Sesekali, Duration::from_secs(60), Duration::from_secs(3600)));
}

#[test]
fn manual_ignores_presence_cooldown_and_an_open_panel() {
    let mut env = ok_env();
    env.panel_open = true;
    assert!(may_start(&env, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
    env.fullscreen = true;
    assert!(!may_start(&env, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
}

#[test]
fn mati_allows_no_automatic_moment() {
    for moment in [Moment::Idle, Moment::Hour, Moment::Greeting] {
        assert!(!may_start(&ok_env(), moment, MascotPlay::Mati, Duration::from_secs(60), Duration::from_secs(3600)));
    }
}

#[test]
fn sapaan_allows_only_the_greeting_and_the_hour() {
    let e = (ok_env(), Duration::from_secs(60), Duration::from_secs(3600));
    assert!(!may_start(&e.0, Moment::Idle, MascotPlay::Sapaan, e.1, e.2));
    assert!(may_start(&e.0, Moment::Greeting, MascotPlay::Sapaan, e.1, e.2));
}

#[test]
fn a_running_play_stops_on_use_a_shape_change_or_a_clock_jump() {
    let mut env = ok_env();
    env.pointer_near = true;
    assert!(must_stop(&env, Story::MalingAngka, false, false), "the pointer entered the hover zone");
    assert!(must_stop(&ok_env(), Story::MalingAngka, true, false), "the size or style changed");
    assert!(must_stop(&ok_env(), Story::MalingAngka, false, true), "the clock jumped");
    let mut no_clock = ok_env();
    no_clock.clock_on = false;
    assert!(must_stop(&no_clock, Story::MalingAngka, false, false), "a clock story with the clock off");
    assert!(!must_stop(&no_clock, Story::Kelelawar, false, false), "a story that needs no clock carries on");
}

#[test]
fn energy_saver_or_windows_animations_off_stops_a_play_instantly() {
    let mut env = ok_env();
    env.energy_saver = true;
    assert!(instant_stop(&env));
    let mut env = ok_env();
    env.win_anim = false;
    assert!(instant_stop(&env));
    assert!(!instant_stop(&ok_env()));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked ui::play::gate`
Expected: FAIL, `Env` not found.

- [ ] **Step 3: Implement `Env`, `may_start`, `must_stop` and `instant_stop`**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked ui::play::gate`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/play/gate.rs src/ui/play/mod.rs
git commit -m "feat(play): the gates that start and stop a play"
```

---

### Task 7: The island is drawn from the deformed outline

**Files:**
- Modify: `src/ui/render.rs` (near `notch_path`, `shape_geometry`, `paint_island`), `src/ui/dirty.rs`

**Interfaces:**
- Consumes: Task 2's `outline`, Task 5's `Beat`.
- Produces: `Renderer::played_path(&self, pts: &[(f32, f32)]) -> Result<ID2D1PathGeometry>` and `Renderer::outside_island(&self, style: Style, s: ShapeRect, pts: Option<&[(f32,f32)]>) -> Result<ID2D1Geometry>` (the "screen minus island" clip, a geometry group with `D2D1_FILL_MODE_ALTERNATE` of the window rect and the island). `Frame` gains `pub play: Option<PlayFrame>`.

- [ ] **Step 1: Write the failing test** in `src/ui/dirty.rs`'s test module

```rust
#[test]
fn a_running_play_forces_a_full_frame() {
    let (prev, cur) = two_frames_differing_only_by_play();
    assert_eq!(plan(Some(&prev), &cur), Plan::Full);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --locked ui::dirty`
Expected: FAIL, `Frame` has no `play`.

- [ ] **Step 3: Add `PlayFrame` to `Frame`, force `Full` in `dirty::plan`, and implement the two geometry helpers; paint in the spec's order**

Paint order, in `paint_island`: everything behind the island clipped to `outside_island`, then inside-the-dark parts clipped to the island geometry, then the island's fill and bezel from `played_path` when a play runs and from `shape_geometry` otherwise. A play never uses the dirty-rect path.

The mascot in its slot is also clipped to the island geometry, so the Pill's rounded end cuts the corner of its head; on the Notch this is nearly a no-op. Verify by eye at 160 x 28, where the clip bites hardest.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/render.rs src/ui/dirty.rs
git commit -m "feat(render): draw the island from a deformed outline while a play runs"
```

---

### Task 8: The clock, per character, only while a play runs

**Files:**
- Modify: `src/ui/render.rs` (the idle clock's `DrawTextLayout` path)

**Interfaces:**
- Produces: `pub struct DigitFx { pub dx: f32, pub dy: f32, pub rot_deg: f32, pub scale: f32, pub alpha: f32 }` with `pub const IDENTITY: DigitFx` (all offsets zero, `scale` 1.0, `alpha` 1.0), `pub fn per_character_wanted(fx: &[DigitFx]) -> bool`, and a renderer path that draws the clock one character at a time from DirectWrite's own cluster metrics when `Frame::play` is `Some` **and** `per_character_wanted` is true. The clock is 5 characters (`%H:%M`).

When no play runs, the existing single `DrawTextLayout` runs unchanged, so the resting clock is pixel-identical by construction rather than by measurement.

- [ ] **Step 1: Write the failing test** in `src/ui/render.rs`'s test module

```rust
#[test]
fn identity_digit_effects_mean_the_plain_clock_path() {
    assert!(!per_character_wanted(&[DigitFx::IDENTITY; 5]));
    let mut fx = [DigitFx::IDENTITY; 5];
    fx[0].dy = 3.0;
    assert!(per_character_wanted(&fx));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --locked ui::render`
Expected: FAIL, `DigitFx` not found.

- [ ] **Step 3: Implement `DigitFx`, `per_character_wanted` and the per-character draw**

Take each character's x offset from the layout's cluster metrics (`IDWriteTextLayout::HitTestTextPosition`), never from a constant, so tracking and kerning are the layout's. The clock's whole transform follows the island's `Deform` (the Pill can shift and tilt).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --locked ui::render`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/render.rs
git commit -m "feat(render): the clock drawn per character while a play runs"
```

---

### Task 9: Timer, wiring and the app's gates

**Files:**
- Modify: `src/app.rs`

**Interfaces:**
- Consumes: Tasks 4, 5 and 6.
- Produces: `const TIMER_MASCOT_PLAY: usize = 19;`, `App::sync_mascot_timer(&mut self, now: Instant)`, `App::start_play(&mut self, moment: Moment, now: Instant)`, `App::play_env(&self) -> Env`, and `App::play: Option<Play>`.

`sync_mascot_timer` is called from `sync_robot_timers`, `sync_pointer_polls` and `follow_pointer_settings`, and arms the one-shot only when `may_start` holds for `Moment::Idle`. The hour moment rides `TIMER_CLOCK`; the greeting moment arms the timer for the instant the greeting notice closes. `App::animating_at` counts a running play. `adopt_config` ends a play when the style, size, mascot or motion changed.

- [ ] **Step 1: Write the failing tests** in `src/app.rs`'s test module

```rust
#[test]
fn timer_ids_never_collide() { /* the existing test, now including TIMER_MASCOT_PLAY */ }

#[test]
fn the_play_timer_runs_only_when_a_play_could_start() {
    // every combination of mascot_play preset x the gate fields that
    // sync_mascot_timer reads: the timer exists exactly when
    // gate::may_start(.., Moment::Idle, ..) is true.
}

#[test]
fn a_config_change_that_moves_the_island_ends_a_running_play() {
    for change in [Change::Style, Change::NotchW, Change::NotchH, Change::Mascot, Change::Motion] {
        let mut app = app_with_a_running_play();
        app.adopt_config(config_with(change));
        assert!(app.play.is_none(), "{change:?} left a play running");
    }
    let mut app = app_with_a_running_play();
    app.adopt_config(config_with(Change::ClipboardLimit));
    assert!(app.play.is_some(), "an unrelated key ended the play");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked app::tests`
Expected: FAIL, `TIMER_MASCOT_PLAY` not found.

- [ ] **Step 3: Implement the field, the timer, the sync and the `adopt_config` hook**

Follow `sync_typing_timer` exactly: a one-shot, killed the moment a gate stops holding, never a repeating timer.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(play): arm the play from the island's own gates"
```

---

### Task 10: Clicks pass through the animation

**Files:**
- Modify: `src/app.rs` (the island window's hit handling)

**Interfaces:**
- Produces: `pub fn swallows(style: Style, s: ShapeRect, playing: bool, x: f32, y: f32) -> bool` in `src/ui/play/mod.rs`.

While a play runs, a point outside the island's own shape is transparent to the mouse, so the click reaches whatever is underneath; the play then winds down.

- [ ] **Step 1: Write the failing test** in `src/ui/play/mod.rs`

```rust
#[test]
fn a_play_never_swallows_a_click_outside_the_island() {
    let s = shape(Style::Notch, 190.0, 34.0);
    assert!(swallows(Style::Notch, s, true, s.x + s.w / 2.0, s.y + 2.0), "the island itself still takes clicks");
    assert!(!swallows(Style::Notch, s, true, s.x + s.w / 2.0, s.y + s.h + 40.0), "a click under the island passes through");
    assert!(!swallows(Style::Notch, s, false, s.x - 60.0, s.y + 10.0));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --locked ui::play`
Expected: FAIL, `swallows` not found.

- [ ] **Step 3: Implement `swallows` and return `HTTRANSPARENT` from the island's hit test for those points; wind the play down on a click**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/ui/play/mod.rs
git commit -m "fix(play): clicks on an animation reach what is underneath"
```

---

### Task 11: "Mainkan maskot" in the island's right-click menu

**Files:**
- Modify: `src/ui/tray.rs`, `src/app.rs` (the `MenuCmd` match near line 8968), `src/i18n.rs`

**Interfaces:**
- Produces: `MenuCmd::PlayMascot`, `COMMANDS` 14 → 15, and `pub fn play_item_shown(env: &Env) -> bool`.

Shown whenever `gate::may_start(env, Moment::Manual, ..)` could hold; present **while the panel is open** (choosing it closes the panel and plays), hidden while Motion is Mati, the mascot is off, a recording runs, a window is fullscreen, the session is locked, or the notch is busy with media or a notice.

- [ ] **Step 1: Write the failing tests** in `src/ui/tray.rs`'s test module

```rust
#[test]
fn the_play_item_survives_an_open_panel_but_not_a_busy_notch() {
    let mut env = ok_env();
    env.panel_open = true;
    assert!(play_item_shown(&env), "open_mode Hover opens the panel before the right-click lands");
    env.panel_open = false;
    env.motion_full = false;
    assert!(!play_item_shown(&env));
    let mut env = ok_env();
    env.idle_view = false;        // media or a notice
    assert!(!play_item_shown(&env));
}
```

The existing `the_menu_ids_are_distinct_and_the_documented_ones` test covers the new id.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked ui::tray`
Expected: FAIL, `MenuCmd::PlayMascot` not found.

- [ ] **Step 3: Add the command, its strings ("Mainkan maskot" / "Play the mascot"), its visibility rule and its handler**

The handler closes the panel if open, then calls `App::start_play(Moment::Manual, now)`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/tray.rs src/app.rs src/i18n.rs
git commit -m "feat(tray): right-click the island to play the mascot"
```

---

### Task 12: The Settings row

**Files:**
- Modify: `src/ui/settings.rs` (the Maskot section)

**Interfaces:**
- Consumes: `MascotPlay`.

One row, "Animasi maskot otomatis", with Sering / Sesekali / Jarang / Hanya sapaan / Mati and a one-line description saying the mascot plays when you are at the PC but not typing, and that right-clicking the island plays one at any time.

- [ ] **Step 1: Write the failing test** in `src/ui/settings.rs`'s test module

Add the row to whichever existing test enumerates the Maskot section's rows, and assert the spacing rules of [[settings-spacing]] hold for it (label + 16 either side, min 64 wide, 30 tall).

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --locked ui::settings`
Expected: FAIL on the row count or the new row's absence.

- [ ] **Step 3: Add the row, wired to `config.mascot_play` through `adopt_config`**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/settings.rs
git commit -m "feat(settings): how often the mascot plays"
```

---

### Task 13: The first story — falk "Maling angka"

The owner's own mascot is the Falk, so this is the story they will see first. It exercises every engine piece: a limb round an end, a dent, the per-character clock, and a `DigitFx` carried by the beak.

**Files:**
- Create: `src/ui/play/stories/mod.rs`, `src/ui/play/stories/maling_angka.rs`
- Modify: `src/ui/play/mod.rs`, `src/ui/render.rs` (call the story painter), `src/main.rs` or wherever `--demo-robot` scenes are listed

**Interfaces:**
- Consumes: `rim`, `Deform`, `Beat`, `DigitFx`, `RobotPainter`.
- Produces: `pub struct Shot { pub deform: Deform, pub digits: [DigitFx; 5], pub mascot: MascotShot, pub props: Vec<Prop> }` and `pub fn shot(style: Style, s: ShapeRect, beat: Beat) -> Shot` per story, behind `stories::shot(story, style, s, beat) -> Shot`.

Read the timeline, the easing and every coordinate from `docs/design/maskot-notch-v3.html`, story id `f2`. Duration 7.8 s.

- [ ] **Step 1: Write the failing tests** in `src/ui/play/stories/maling_angka.rs`

```rust
#[test]
fn the_story_starts_and_ends_at_rest_in_both_styles() {
    for style in [Style::Notch, Style::Pill] {
        for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
            let s = shape(style, w, h);
            for t in [0.0, Story::MalingAngka.secs()] {
                let shot = shot(style, s, Beat { spread: 1.0, alpha: 1.0, t });
                assert!(is_rest(&shot.deform), "{style:?} {w}x{h} t={t}");
                assert!(shot.digits.iter().all(|d| *d == DigitFx::IDENTITY), "{style:?} {w}x{h} t={t}");
            }
        }
    }
}

#[test]
fn alpha_zero_means_no_deformation_at_any_moment() {
    let s = shape(Style::Pill, 190.0, 34.0);
    for i in 0..=78 {
        let beat = Beat { spread: 1.0, alpha: 0.0, t: i as f32 / 10.0 };
        assert!(is_rest(&shot(Style::Pill, s, beat).deform), "t={}", beat.t);
    }
}

#[test]
fn the_stolen_digit_is_carried_and_then_returned() {
    let s = shape(Style::Notch, 190.0, 34.0);
    let mid = shot(Style::Notch, s, Beat { spread: 1.0, alpha: 1.0, t: 4.0 });
    assert!(mid.digits.iter().any(|d| d.dx.abs() > 5.0), "a digit has left its place");
    let end = shot(Style::Notch, s, Beat { spread: 1.0, alpha: 1.0, t: 7.6 });
    assert!(end.digits.iter().all(|d| d.dx.abs() < 1.0), "every digit is home before the end");
}

#[test]
fn nothing_leaves_the_window_at_any_size_or_style() {
    for style in [Style::Notch, Style::Pill] {
        for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
            let s = shape(style, w, h);
            let mut t = 0.0;
            while t <= Story::MalingAngka.secs() {
                let shot = shot(style, s, Beat { spread: 1.0, alpha: 1.0, t });
                let b = shot.bounds();
                assert!(b.x >= 0.0 && b.y >= 0.0 && b.x + b.w <= WINDOW_W && b.y + b.h <= WINDOW_H,
                    "{style:?} {w}x{h} t={t}: {b:?}");
                t += 0.04;
            }
        }
    }
}
```

`Shot::bounds() -> Rect` is the union of everything the shot paints, in window coordinates; the nine stories of part 2 reuse this test verbatim.

The falk's new wings must take their colours from the same `theme::Coat` derivation its head already uses, so all three falk colours work, and the chosen accessory stays on and follows the head for the whole story ([[cat-accessories]], [[maskot-falk-gurita]]). Check Lutino, Grey and WhiteFace in step 5.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --locked ui::play::stories`
Expected: FAIL, `shot` not found.

- [ ] **Step 3: Implement `Shot`, `MascotShot`, `Prop` and the story's timeline, and paint it from the renderer**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --locked`
Expected: PASS.

- [ ] **Step 5: Look at it running, in both styles**

Run: `cargo run --release -- --demo-robot maling-angka` (and with `DI_STYLE=pill`), at 160x28, 190x34 and 300x44.
Expected: the falk taps, steals the digit, the clock regrows it, the crest shoots up and droops; nothing jumps at the start or the end; the island is exactly at rest before and after.

- [ ] **Step 6: Commit**

```bash
git add src/ui/play/stories src/ui/play/mod.rs src/ui/render.rs
git commit -m "feat(play): the falk steals a clock digit"
```

---

## After this plan

Part 2 is the remaining nine stories: Petak umpet, Bantal angka, Piano ekor, Syal ekor (cat), Penasaran jam, Peluk pulau, Bungee (octopus), Kelelawar, Terbang keliling (falk). Each is one task of the same shape as Task 13 — a timeline module, the four tests, a demo scene — and they are independent of one another, so they can run in parallel once this plan is green. Write that plan after this one merges, so it is written against the engine as built.
