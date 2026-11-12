# Falk and sitting-octopus mascots Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user pick the notch mascot (cat, falk, sitting octopus), each with its own coats, the shared accessories (three new ones) and every cat face and motion, at zero idle cost.

**Architecture:** The cat's painter path stays the one path: `CatLook` grows the mascot and its coats, `RobotPainter::paint` dispatches the body (silhouette) per mascot and keeps the face marks, accessories, laptop and paws flow. The new mascots' geometry is pure data in a new `src/ui/mascot.rs` (paths as `Seg` lists, per-face poses, test outlines), painted by an `impl RobotPainter` block in the same file, like `cat_style.rs`. One-shot motions (hop, wave, flush) live in `Brain` next to the alert bounce and ride the frame clock.

**Tech Stack:** Rust 2024, `windows` crate Direct2D (`ID2D1PathGeometry`, `Matrix3x2`), serde config, the project's own test helpers (`painter()` in `cat_style` tests, `drawn`/`shape_contains` in `geometry.rs`).

**Spec:** `docs/superpowers/specs/2026-11-12-maskot-falk-gurita-design.md` (read it first; every coordinate and colour below comes from it). Reference drawings: `docs/design/maskot-falk-gurita-animasi.html`, `maskot-falk-sayap.html` (falk "Pilihan 1"), `maskot-pilihan-akhir.html`; their JS is the visual truth for shapes.

## Global Constraints

- Windows 11 x64, Rust edition 2024; build `cargo build --release --locked`, test `cargo test --locked`.
- No new repeating timer, no new timer id, no new thread, nothing moving at idle ([[bata-idle-power-budget]]).
- The cat's drawing, geometry, accessories and every existing test value stay exactly as they are.
- Paths are built once and cached (like `RobotPainter::path` / `extras`); nothing rebuilt per frame.
- Config is lenient: an unknown or wrong-typed value falls back per field, old files load unchanged; new keys sit at the end of `Config` and in `merge_owned`.
- Enum values are appended, never reordered (`CatAccessory`: None, Glasses, Bow, Nightcap, Scarf, Flower, Crown, **Headphone, Sailor, Bowtie**).
- UI text in Indonesian and English (`i18n.rs`), no em dash in any string.
- Logo, exe icon, tray, notch-peek eyes, landing page and the sticky-note cat peek (`widgets/paint.rs::peek_look`) stay the cat.
- Tests spawn no visible window ([[test-spawns-no-window]]).
- Never use the Fable model for any subagent; name the model (Haiku, Sonnet, Opus) on every dispatch.

## Review Focus

- An old `config.json` with none of the new keys, or junk in them (`"mascot": 3`, `"falk_color": "Pink"`): loads as Cat / Lutino / Coral with every other field kept. Task 1 pins it.
- A stored accessory name from before ("Crown", "None") and the three new names read back exactly; `CatAccessory::ALL` order unchanged for the first seven. Task 1 pins it.
- Switching mascot while a pose carries flush, hop, wave, beat or paws (e.g. an alert on the octopus, then Settings → Kucing): the cat paints exactly its usual colours and nothing of the others' fields. Task 6 pins it with a pixel test.
- Motion Saver or Off, Energy Saver, Windows animations off: no hop, wave or flush frames, `moving()` false, poses still. Task 5 pins it.
- The smallest notch (160 × 28, both styles) on the falk's Alert crest and the octopus's wave arm: inside the island. Task 7 pins it.
- Settings at its minimum size (720 × 520): the ten accessory tiles and the three mascot tiles do not overlap or leave the card. Task 8 pins it.

---

### Task 1: Mascot, coats and accessories as config data

**Files:**
- Modify: `src/theme.rs` (enums near `CatColor`, `Surface::resolve`)
- Modify: `src/config.rs` (fields at the end of `Config`, readers like `cat_color`, `merge_owned`)
- Modify: every caller of `surface.resolve(cfg.cat_color, ..)` (6 sites: `theme.rs`, `settings_layout.rs`, demo, render; find with `rg "resolve\(" src`)

**Interfaces:**
- Produces (in `theme.rs`):
  - `named_enum! { Mascot { Cat, Falk, Octopus } }` (default Cat)
  - `named_enum! { FalkColor { Lutino, Grey, WhiteFace } }` (default Lutino)
  - `named_enum! { OctopusColor { Coral, Red, Purple, Blue } }` (default Coral)
  - `CatAccessory` gains `Headphone, Sailor, Bowtie` at the end.
  - `pub enum Coat { Cat(CatColor), Falk(FalkColor), Octopus(OctopusColor) }` with `pub fn Coat::of(cfg: &Config) -> Coat` (the chosen mascot's own coat).
  - `Surface::resolve(self, coat: Coat, robot_on: bool) -> Surface` (replaces the `CatColor` argument).
- Produces (in `config.rs`): `pub mascot: Mascot`, `pub falk_color: FalkColor`, `pub octopus_color: OctopusColor`.

- [ ] **Step 1: Write the failing tests** (in `theme.rs` and `config.rs` test modules)

```rust
#[test]
fn auto_follows_every_mascots_coat() {
    use Coat::*;
    let cases = [
        (Cat(CatColor::Terracotta), Surface::Espresso), (Cat(CatColor::Calico), Surface::Sea),
        (Cat(CatColor::White), Surface::Night), (Cat(CatColor::Black), Surface::Plum),
        (Falk(FalkColor::Lutino), Surface::Espresso), (Falk(FalkColor::Grey), Surface::Night),
        (Falk(FalkColor::WhiteFace), Surface::Plum),
        (Octopus(OctopusColor::Coral), Surface::Espresso), (Octopus(OctopusColor::Red), Surface::Graphite),
        (Octopus(OctopusColor::Purple), Surface::Night), (Octopus(OctopusColor::Blue), Surface::Sea),
    ];
    for (coat, want) in cases {
        assert_eq!(Surface::Auto.resolve(coat, true), want, "{coat:?}");
        assert_eq!(Surface::Auto.resolve(coat, false), Surface::Espresso);
        assert_ne!(Surface::Auto.resolve(coat, true), Surface::Terminal);
        assert_eq!(Surface::Sea.resolve(coat, true), Surface::Sea);
    }
}

#[test]
fn accessories_keep_their_order_and_names() {
    let names: Vec<_> = CatAccessory::ALL.iter().map(|a| a.name()).collect();
    assert_eq!(names, ["None", "Glasses", "Bow", "Nightcap", "Scarf", "Flower", "Crown", "Headphone", "Sailor", "Bowtie"]);
}

#[test] // config.rs
fn mascot_keys_are_lenient_and_default_to_the_cat() {
    let c: Config = serde_json::from_str(r#"{"cat_color":"Calico"}"#).unwrap();
    assert_eq!((c.mascot, c.falk_color, c.octopus_color), (Mascot::Cat, FalkColor::Lutino, OctopusColor::Coral));
    assert_eq!(c.cat_color, CatColor::Calico);
    let c: Config = serde_json::from_str(r#"{"mascot":3,"falk_color":"Pink","octopus_color":null,"cat_accessory":"Sailor"}"#).unwrap();
    assert_eq!((c.mascot, c.falk_color, c.octopus_color), (Mascot::Cat, FalkColor::Lutino, OctopusColor::Coral));
    assert_eq!(c.cat_accessory, CatAccessory::Sailor);
    let c: Config = serde_json::from_str(r#"{"mascot":"Octopus","octopus_color":"Blue"}"#).unwrap();
    assert_eq!(Coat::of(&c), Coat::Octopus(OctopusColor::Blue));
}
```
Also add the three keys to the existing `merge_owned` round-trip test (the island owns them).

- [ ] **Step 2: Run them, expect compile errors** — `cargo test --locked -p vale auto_follows_every_mascots_coat` (and the other two names). Expected: unresolved `Coat`, `Mascot`.
- [ ] **Step 3: Implement** the enums, `Coat::of`, the new `resolve`, the three lenient readers (copy `cat_color`'s shape), the fields with `#[serde(default, deserialize_with = ..)]`, `merge_owned`; update the resolve call sites to `Coat::of(cfg)` (Settings preview resolves from its own model's mascot and coat the same way).
- [ ] **Step 4: Run** `cargo test --locked` — all green (existing `every_concrete_surface_*` and pack tests untouched).
- [ ] **Step 5: Commit** `feat(mascot): mascot and coat config, Auto surface per coat`

---

### Task 2: The falk's geometry (pure)

**Files:**
- Create: `src/ui/mascot.rs` (declare in `src/ui/mod.rs`)
- Modify: `src/ui/robot.rs` (`Pose` gains the fields below)

**Interfaces:**
- Consumes: `Expr`, `Seg`, `Pose` (`robot.rs`), `FalkColor` (Task 1).
- Produces:
  - `pub const FALK_BODY: &[Seg]` (the egg path), `pub fn feather(base: (f32,f32), tip: (f32,f32), half_w: f32, bend: f32) -> Vec<Seg>` (the spec's quad leaf), `pub const CREST: [((f32,f32),(f32,f32),f32,f32); 4]` (the four feathers), `pub const CREST_PIVOT: (f32,f32) = (32.0, 27.0)`.
  - `pub struct CrestPose { pub rot_deg: f32, pub scale: f32 }` and `pub fn crest_pose(expr: Expr, pose: &Pose) -> CrestPose`.
  - `pub struct FalkInk { pub body: u32, pub face: u32, pub crest: u32, pub cheek: Option<u32> }`, `pub fn falk_ink(c: FalkColor) -> FalkInk`; `pub const FALK_BEAK: u32 = 0xD9978F`.
  - `pub fn falk_beak(open: bool) -> Vec<Vec<Seg>>` (one closed path closed, two open); `pub fn beak_open(expr: Expr, pose: &Pose) -> bool` (Sing, Music while `pose.dancing`, Alert).
  - `pub const FALK_CHEEKS: [(f32,f32); 2] = [(15.5,44.5),(48.5,44.5)]`, `pub const CHEEK_R: f32 = 4.4`, `pub const FALK_FACE: (f32,f32,f32,f32) = (32.0,33.0,22.0,17.0)` (ellipse cx, cy, rx, ry).
  - `pub fn feather_tip(x: f32, y: f32) -> Vec<Seg>` (the typing tip, spec path).
  - `pub fn falk_outline(expr: Expr, pose: &Pose) -> Vec<(f32,f32)>`: grid points of the body (sampled, ≥ 48) and every crest feather's quad (sampled, its half-width included) after `crest_pose`; for the clamp tests.
- `Pose` gains (in `robot.rs`, all `Default` 0 / false): `pub beat: f32` (the dance's phase in radians, `π · elapsed / BEAT_MS`), `pub hop_px: f32`, `pub wave: f32` (−1..1 swing, 0 at rest), `pub waving: bool`, `pub flush: f32` (0..1). Task 5 fills them; here only `beat` and `waving`/`wave` are read.

- [ ] **Step 1: Failing tests** in `mascot.rs`:

```rust
#[test]
fn the_crest_carries_the_mood() {
    let p = Pose::default();
    let c = |e| crest_pose(e, &p);
    assert_eq!((c(Expr::Idle).rot_deg, c(Expr::Idle).scale), (10.0, 1.0));
    assert_eq!((c(Expr::Happy).rot_deg, c(Expr::Happy).scale), (2.0, 1.08));
    assert_eq!((c(Expr::Alert).rot_deg, c(Expr::Alert).scale), (-4.0, 1.22));
    assert_eq!((c(Expr::Sleep).rot_deg, c(Expr::Sleep).scale), (62.0, 0.8));
    assert_eq!((c(Expr::Sad).rot_deg, c(Expr::Sad).scale), (46.0, 0.75));
    for e in [Expr::Focus, Expr::Think, Expr::Typing] { assert_eq!(c(e).rot_deg, 10.0); }
    assert_eq!(c(Expr::Music).rot_deg, 6.0); // at rest, not dancing
    let dancing = Pose { dancing: true, beat: std::f32::consts::FRAC_PI_2, ..Pose::default() };
    assert!((crest_pose(Expr::Music, &dancing).rot_deg - 15.0).abs() < 1e-4); // 6 + 9·sin
    let waving = Pose { waving: true, wave: -1.0, ..Pose::default() };
    assert!((crest_pose(Expr::Wave, &waving).rot_deg - 0.0).abs() < 1e-4); // 10 + 10·wave
}

#[test]
fn falk_coats_are_the_specs() {
    assert_eq!(falk_ink(FalkColor::Lutino), FalkInk { body: 0xF1E4A2, face: 0xF6D04A, crest: 0xF6D04A, cheek: Some(0xEC6B3A) });
    assert_eq!(falk_ink(FalkColor::Grey), FalkInk { body: 0x9A959C, face: 0xF2D35A, crest: 0xE9CF62, cheek: Some(0xEC6B3A) });
    assert_eq!(falk_ink(FalkColor::WhiteFace), FalkInk { body: 0x9A959C, face: 0xF4EFE6, crest: 0xE4DED6, cheek: None });
}

#[test]
fn the_falk_outline_holds_the_crest_tip_and_the_body() {
    let o = falk_outline(Expr::Idle, &Pose::default());
    let top = o.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    let bottom = o.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    assert!(top < 8.0, "the crest reaches near the grid top, got {top}");
    assert!((bottom - 57.0).abs() < 0.5);
    // the Alert crest is the tallest
    let alert = falk_outline(Expr::Alert, &Pose::default()).iter().map(|p| p.1).fold(f32::MAX, f32::min);
    assert!(alert < top);
}

#[test]
fn the_beak_opens_to_sing_and_on_alert() {
    assert!(beak_open(Expr::Sing, &Pose::default()));
    assert!(beak_open(Expr::Alert, &Pose::default()));
    assert!(!beak_open(Expr::Music, &Pose::default()));
    assert!(beak_open(Expr::Music, &Pose { dancing: true, ..Pose::default() }));
    assert_eq!(falk_beak(false).len(), 1);
    assert_eq!(falk_beak(true).len(), 2);
}
```
- [ ] **Step 2: Run** `cargo test --locked mascot::` — fails to compile.
- [ ] **Step 3: Implement** the data and functions with the spec's numbers; crest sway = `9 · sin(beat)` only when `pose.dancing` and the face is Music/Sing; wave = `10 · wave` only when `pose.waving` and the face is Wave.
- [ ] **Step 4: Run** `cargo test --locked mascot::` — pass.
- [ ] **Step 5: Commit** `feat(mascot): falk geometry and crest moods`

---

### Task 3: The octopus's geometry (pure)

**Files:**
- Modify: `src/ui/mascot.rs`

**Interfaces:**
- Consumes: Task 2's module, `Pose`, `OctopusColor`.
- Produces:
  - `pub const MANTLE: &[Seg]`, `pub const OCT_SPOTS: [(f32,f32,f32); 3]`, `pub const OCT_EYE_LIFT: f32 = -3.0`.
  - `pub struct Arm { pub start: (f32,f32), pub ctrl: (f32,f32), pub end: (f32,f32), pub curl_side: f32, pub curl: f32 }` and `pub fn arms(expr: Expr, pose: &Pose) -> [Arm; 6]` (index 0..2 left outer→inner, 3..5 right outer→inner).
  - `pub fn arm_segs(a: &Arm) -> Vec<Seg>` (quad + the two curl quads of the spec); `pub fn suckers(a: &Arm) -> [(f32,f32); 2]` (t 0.55 and 0.8, +1.6 y); `pub const ARM_W: f32 = 5.0`, `pub const SUCKER_R: f32 = 0.85`.
  - `pub fn octopus_rgb(c: OctopusColor) -> u32` (Coral 0xE87C8E, Red 0xD9505F, Purple 0xA98BD6, Blue 0x5FA8C9); `pub fn shade(rgb) -> u32` (×0.82 toward black), `pub fn sucker_rgb(rgb) -> u32` (55 % toward white), `pub const FLUSH_RGB: u32 = 0xD63A55`, `pub fn mix(a: u32, b: u32, t: f32) -> u32`.
  - `pub fn octopus_outline(expr: Expr, pose: &Pose) -> Vec<(f32,f32)>`: mantle samples plus every arm's quad and curl samples with `ARM_W / 2` round caps, and the suckers.
  - Typing arms read `pose.paw_lift` (the outer two left arms, spec values); Wave reads `pose.waving` / `pose.wave`; dancing reads `pose.dancing` / `pose.beat` with arm sway `sin(beat · 600/190 + i · 1.4 + side)` (side 0 left, 1 right; this keeps the mockup's 190 ms rhythm on the beat clock).
  - Sleep uses the coiled arms (the chosen extra): curl 1.8, end 4 toward the middle, 1 up.

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn octopus_arms_mirror_and_rest_on_the_floor() {
    let a = arms(Expr::Idle, &Pose::default());
    assert_eq!(a[0].start, (19.0, 42.0)); assert_eq!(a[0].end, (3.0, 54.0));
    assert_eq!(a[2].end, (21.0, 58.5));
    for i in 0..3 { assert_eq!(a[i + 3].start.0, 64.0 - a[i].start.0); assert_eq!(a[i + 3].end, (64.0 - a[i].end.0, a[i].end.1)); }
    assert!(a.iter().all(|x| x.curl == 1.0));
}

#[test]
fn octopus_arms_carry_the_mood() {
    let p = Pose::default();
    assert!(arms(Expr::Happy, &p).iter().all(|x| x.curl == 1.25));
    assert!(arms(Expr::Alert, &p)[0].end.0 < 3.0 && arms(Expr::Alert, &p)[0].curl == 1.3);
    assert!(arms(Expr::Sad, &p).iter().all(|x| x.curl == 0.4));
    assert!(arms(Expr::Sleep, &p).iter().all(|x| x.curl == 1.8)); // coiled
    let wave = arms(Expr::Wave, &Pose { waving: true, wave: 0.0, ..p });
    assert_eq!(wave[3].end, (60.0, 28.0));
    let typing = arms(Expr::Typing, &Pose { paw_lift: [1.6, 0.0], ..p });
    assert_eq!(typing[0].end, (1.0, 53.0 - 1.6)); assert_eq!(typing[1].end, (9.5, 53.0));
}

#[test]
fn octopus_coats_and_tints() {
    assert_eq!(octopus_rgb(OctopusColor::Coral), 0xE87C8E);
    assert_eq!(octopus_rgb(OctopusColor::Blue), 0x5FA8C9);
    assert_eq!(mix(0xE87C8E, FLUSH_RGB, 0.0), 0xE87C8E);
    assert_eq!(mix(0xE87C8E, FLUSH_RGB, 1.0), FLUSH_RGB);
}

#[test]
fn the_octopus_outline_holds_the_mantle_and_the_curls() {
    let o = octopus_outline(Expr::Idle, &Pose::default());
    let (l, r) = o.iter().fold((f32::MAX, f32::MIN), |(l, r), p| (l.min(p.0), r.max(p.0)));
    assert!(l < 3.0 && r > 61.0, "arms reach the sides: {l}..{r}");
    assert!(o.iter().any(|p| p.1 < 10.0)); // the mantle top
}
```
- [ ] **Step 2: Run** — fails. **Step 3: Implement.** **Step 4: Run** `cargo test --locked mascot::` — pass.
- [ ] **Step 5: Commit** `feat(mascot): octopus geometry and arm moods`

---

### Task 4: Accessories per mascot, three new accessories

**Files:**
- Modify: `src/ui/cat_style.rs` (`pieces`, `accessory_corners`, `extras` cache key, `CatLook`)

**Interfaces:**
- Consumes: `Mascot` (Task 1), the falk and octopus anchors (spec table).
- Produces:
  - `CatLook { pub mascot: Mascot, pub color: CatColor, pub falk: FalkColor, pub octopus: OctopusColor, pub accessory: CatAccessory }` (Default: Cat, today's values); `CatLook::of(cfg)` fills all five.
  - `pub struct Anchors { pub top: (f32,f32), pub left: (f32,f32), pub right: (f32,f32), pub band: (f32,f32), pub tie: (f32,f32), pub hp: (f32,f32,f32,f32), pub eye_lift: f32 }` and `pub fn anchors(m: Mascot) -> Anchors` (cat: top (32,24), left (16,23), right (45,23), band (47,55) as today's scarf, tie (32,51.5), hp (7,57,41,16), eye_lift 0; falk and octopus from the spec table, octopus eye_lift −3).
  - `pub fn pieces(m: Mascot, accessory: CatAccessory) -> Vec<(Anchor, Piece)>`: for `Mascot::Cat` and the first seven accessories exactly today's pieces. For the falk and the octopus each of today's pieces is moved by (its anchor − the cat's anchor), with these exceptions from the spec: Scarf is redrawn as the band of `anchors(m).band` (falk clipped by drawing it as the band between the body's sides at those y values, i.e. the body's x span at y0..y1; octopus a rounded band x 14..50) plus its tail at x 13 (falk) or 17 (octopus), falk scarf colour NAVY; the falk's Crown gets a 0.9 #7A5210 outline piece; Ear-anchored pieces become `Anchor::Head` on the falk and the octopus. Glasses keep `Anchor::Eyes` (the painter adds the eye lift).
  - The new three, built from `anchors(m)` for every mascot (cat included): Headphone, Sailor, Bowtie as the spec describes (cream #F4EDE4, NAVY, #F6F1EA, knot #5878AE).
  - `accessory_corners(m: Mascot, accessory, ears) -> Vec<((f32,f32), bool)>` (the clamp test's input; ears only matter for the cat).
  - The `extras` cache keyed by `(Mascot, CatAccessory)`.

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn the_cats_accessories_are_unchanged() {
    // A literal copy of today's `pieces(acc)` for the seven, kept in the test as `old_pieces`.
    for acc in &CatAccessory::ALL[..7] { assert_eq!(pieces(Mascot::Cat, *acc), old_pieces(*acc), "{acc:?}"); }
}

#[test]
fn every_mascot_wears_every_accessory() {
    for m in Mascot::ALL { for acc in CatAccessory::ALL {
        let p = pieces(*m, *acc);
        assert_eq!(p.is_empty(), *acc == CatAccessory::None, "{m:?} {acc:?}");
        if *m != Mascot::Cat { assert!(p.iter().all(|(a, _)| !matches!(a, Anchor::Ear(_))), "{m:?} {acc:?} rides no ear"); }
    }}
}

#[test]
fn caps_sit_on_each_mascots_top() {
    for m in Mascot::ALL {
        let (_, t, _, b) = bounds_of(&pieces(*m, CatAccessory::Sailor));
        let top = anchors(*m).top.1;
        assert!(t >= top - 9.5 && b <= top + 3.5, "{m:?}: {t}..{b} around {top}");
    }
    assert_eq!(anchors(Mascot::Octopus).top, (32.0, 9.0));
}

#[test]
fn the_falk_crown_is_outlined_and_its_scarf_navy() {
    let crown = pieces(Mascot::Falk, CatAccessory::Crown);
    assert!(crown.iter().any(|(_, p)| matches!(p, Piece::Stroke { tone: Tone::Fixed(0x7A5210), .. } | Piece::Shape { fill: Tone::Fixed(0x7A5210), .. })));
    let scarf = pieces(Mascot::Falk, CatAccessory::Scarf);
    assert!(scarf.iter().any(|(_, p)| matches!(p, Piece::Shape { fill: Tone::Fixed(NAVY), .. })));
}
```
(`bounds_of` = union of `piece_bounds` over the pieces; add it next to `piece_bounds`.) Extend `no_accessory_covers_a_faces_cream_extras` and `accessory_paths_are_built_once` to loop over `Mascot::ALL`.
- [ ] **Step 2: Run** `cargo test --locked cat_style::` — fails.
- [ ] **Step 3: Implement**; update every `CatLook { color, accessory }` literal (`rg "CatLook \{" src`) to `..CatLook::default()` with the mascot where known; `peek_look` stays `Mascot::Cat`.
- [ ] **Step 4: Run** `cargo test --locked` — green.
- [ ] **Step 5: Commit** `feat(mascot): accessories per mascot, headphone, sailor cap and bow tie`

---

### Task 5: One-shot motions in the brain (hop, wave, flush, beat)

**Files:**
- Modify: `src/ui/robot.rs` (`Brain`, constants, `Pose`)
- Modify: `src/app.rs` (`observe` call at ~3812, `robot_frame` ~1963, the dance block that sets `Pose::dance`, `robot_moving` ~2024)

**Interfaces:**
- Consumes: `Mascot` (Task 1), `Pose` fields from Task 2.
- Produces:
  - `pub const HOP_PX: f32 = 1.25; pub const HOP_MS: u64 = 400; pub const WAVE_MS: u64 = 900; pub const WAVE_SWINGS: u32 = 3; pub const FLUSH_MS: u64 = 700;`
  - `pub fn hop_offset(elapsed: Duration) -> f32` (`HOP_PX · sin(π t/HOP_MS)`, 0 after), `pub fn wave_swing(elapsed: Duration) -> Option<f32>` (`Some(sin(2π · swings · t/WAVE_MS))` while under `WAVE_MS`, then `None` = held), `pub fn flush_amount(elapsed: Duration) -> f32` (`sin(π t/FLUSH_MS)`, 0 after).
  - `Brain::observe(&mut self, expr: Option<Expr>, notice: Option<u64>, now: Instant, moves: bool) -> bool`: `notice` is a token that changes with every notice shown (the builder adds a `u64` counter to the state or `App` bumped whenever a notice becomes the shown one, if no such id exists). A new token with face Happy starts the hop; with a Greeting face (`NoticeSort::Greeting`, the Wave face) starts the wave; the alert bounce keeps its face-edge rule and also stamps the flush start. Hover never starts a hop.
  - `Brain::pose(&self, spot, now, moves, blink, mascot: Mascot) -> Pose`: adds `hop_px` (falk only), `waving`/`wave` (falk and octopus; `waving` stays true with `wave` 0 after the swings, so the raised arm holds), `flush` (octopus only).
  - `Brain::moving(&self, spot, now, moves, mascot) -> bool`: also true while the hop (falk), the wave (falk, octopus) or the flush (octopus, up to 700 ms) runs.
  - The app sets `pose.beat = π · dance_elapsed_ms / BEAT_MS` wherever it builds `Pose::dance` (same anchor `App::dance_anchor`).

- [ ] **Step 1: Failing tests** (robot.rs tests, next to the bounce tests):

```rust
#[test]
fn the_falk_hops_once_per_happy_notice() {
    let t0 = Instant::now();
    let mut b = Brain::new(t0);
    let n = Spot::Notice(NoticeSort::ClaudeDone);
    b.observe(Some(Expr::Happy), Some(1), t0, true);
    let at = |ms| b.pose(n, t0 + Duration::from_millis(ms), true, false, Mascot::Falk).hop_px;
    assert!((at(200) - HOP_PX).abs() < 1e-3);
    assert_eq!(at(400), 0.0);
    assert!(b.moving(n, t0 + Duration::from_millis(399), true, Mascot::Falk));
    assert!(!b.moving(n, t0 + Duration::from_millis(400), true, Mascot::Falk));
    assert_eq!(b.pose(n, t0 + Duration::from_millis(200), true, false, Mascot::Cat).hop_px, 0.0);
    assert!(!b.moving(n, t0 + Duration::from_millis(200), true, Mascot::Cat));
    // a second Happy notice restarts it
    let t1 = t0 + Duration::from_secs(2);
    b.observe(Some(Expr::Happy), Some(2), t1, true);
    assert!(b.pose(n, t1 + Duration::from_millis(200), true, false, Mascot::Falk).hop_px > 1.0);
    // the same notice again does not
    b.observe(Some(Expr::Happy), Some(2), t1 + Duration::from_secs(1), true);
    assert_eq!(b.pose(n, t1 + Duration::from_millis(1200), true, false, Mascot::Falk).hop_px, 0.0);
}

#[test]
fn hover_and_saver_never_hop_wave_or_flush() {
    let t0 = Instant::now();
    let mut b = Brain::new(t0);
    b.hover(true, t0);
    b.observe(Some(Expr::Happy), None, t0, true);
    assert_eq!(b.pose(Spot::Idle, t0 + Duration::from_millis(200), true, false, Mascot::Falk).hop_px, 0.0);
    let mut s = Brain::new(t0);
    s.observe(Some(Expr::Alert), Some(5), t0, false);
    let p = s.pose(Spot::Notice(NoticeSort::Reminder), t0 + Duration::from_millis(300), false, false, Mascot::Octopus);
    assert_eq!((p.flush, p.hop_px, p.waving), (0.0, 0.0, false));
    assert!(!s.moving(Spot::Notice(NoticeSort::Reminder), t0, false, Mascot::Octopus));
}

#[test]
fn the_octopus_flushes_for_700_ms_on_alert_and_the_bounce_keeps_its_numbers() {
    let t0 = Instant::now();
    let mut b = Brain::new(t0);
    let r = Spot::Notice(NoticeSort::Reminder);
    assert!(b.observe(Some(Expr::Alert), Some(9), t0, true));
    let at = |ms, m| b.pose(r, t0 + Duration::from_millis(ms), true, false, m);
    assert!((at(350, Mascot::Octopus).flush - 1.0).abs() < 1e-3);
    assert_eq!(at(350, Mascot::Cat).flush, 0.0);
    assert!(b.moving(r, t0 + Duration::from_millis(650), true, Mascot::Octopus));
    assert!(!b.moving(r, t0 + Duration::from_millis(650), true, Mascot::Cat)); // the cat's 520 ms
    assert_eq!(at(130, Mascot::Octopus).bounce_px, at(130, Mascot::Cat).bounce_px);
}

#[test]
fn the_greeting_waves_three_times_then_holds() {
    let t0 = Instant::now();
    let mut b = Brain::new(t0);
    let g = Spot::Notice(NoticeSort::Greeting(crate::greeting::Period::Morning)); // the builder uses the real period type
    b.observe(Some(Expr::Wave), Some(3), t0, true);
    let p = b.pose(g, t0 + Duration::from_millis(75), true, false, Mascot::Octopus);
    assert!(p.waving && (p.wave - 1.0).abs() < 1e-3);
    let held = b.pose(g, t0 + Duration::from_millis(950), true, false, Mascot::Octopus);
    assert!(held.waving && held.wave == 0.0);
    assert!(!b.moving(g, t0 + Duration::from_millis(950), true, Mascot::Falk));
    assert!(!b.pose(g, t0 + Duration::from_millis(75), true, false, Mascot::Cat).waving);
}
```
Update the existing `observe` and `pose` call sites in tests (`b.observe(x, None, t, m)`, `pose(.., Mascot::Cat)`): their expected values stay.
- [ ] **Step 2: Run** `cargo test --locked robot::` — fails.
- [ ] **Step 3: Implement**; in `app.rs` pass the notice token and `self.config.mascot`, set `pose.beat`.
- [ ] **Step 4: Run** `cargo test --locked` — green.
- [ ] **Step 5: Commit** `feat(mascot): hop, greeting wave and flush on the frame clock`

---

### Task 6: Painting the falk and the octopus

**Files:**
- Modify: `src/ui/mascot.rs` (an `impl RobotPainter` block: `paint_falk_body`, `paint_octopus_body`, `paint_falk_tips`)
- Modify: `src/ui/robot.rs` (`RobotPainter::paint` dispatch; `Shape` gains cached variants for the new paths, or a second cache keyed by an own enum in `mascot.rs`)
- Modify: `src/ui/render.rs` (`paint_robot` halo, ~2371)

**Interfaces:**
- Consumes: Tasks 2-5.
- Produces:
  - In `paint`: when `look.mascot != Cat`, steps 1-3 (rim, cat silhouette, calico) are replaced by the mascot's body in the same `world` transform (falk: crest group rotated/scaled by `crest_pose` round `CREST_PIVOT`, body in `ink.body`, the face ellipse in `ink.face` clipped to the body (a layer with the body geometry as mask, or the face ellipse INTERSECT body built once), the cheeks with the eyes' transform at half slide; octopus: the six arms (stroke `ARM_W`, round), the mantle, spots, suckers; colour `mix(rgb, FLUSH_RGB, pose.flush)` for mantle and arms). The face marks use `look.eye_rgb()` (espresso for every falk and octopus coat) and the eyes transform translated by `anchors(m).eye_lift` in y. The falk's beak is drawn with the eyes' transform after the eye marks. The whole mascot is raised by `pose.hop_px` like `bounce_px`. Paws: the cat as today; the falk draws `feather_tip` at `PAWS` in `ink.body`; the octopus draws none (its arms type). The laptop is unchanged for all.
  - `pub fn mascot_box(m: Mascot, area: Rect, pose: &Pose) -> (Rect, f32)` in `mascot.rs` (cat: `head_rect`; falk: body x 9..55, y crest top..57; octopus: x 1..63, y 9..61; radius like the head's) used by the halo instead of `head_rect`.

- [ ] **Step 1: Failing tests** (in `mascot.rs`, using the WIC test target the `cat_style` tests use; add a helper `render(look, expr, pose) -> image` that paints into a 64 × 64 px area and `px(img, x, y) -> u32`):

```rust
#[test]
fn the_falk_paints_its_coat_and_face() {
    let img = render(falk(FalkColor::Lutino), Expr::Idle, Pose::default());
    assert_near(px(&img, 15, 44), 0xEC6B3A); // a cheek
    assert_near(px(&img, 32, 28), 0xF6D04A); // the face mask
    assert_near(px(&img, 32, 55), 0xF1E4A2); // the body below the mask
    assert_near(px(&img, 24, 40), 0x17120F); // an eye
    assert_near(px(&img, 32, 46), 0xD9978F); // the beak
    let white = render(falk(FalkColor::WhiteFace), Expr::Idle, Pose::default());
    assert_near(px(&white, 15, 44), 0xF4EFE6); // no cheek on Muka putih
}

#[test]
fn the_octopus_paints_low_eyes_arms_and_flush() {
    let img = render(octopus(OctopusColor::Coral), Expr::Idle, Pose::default());
    assert_near(px(&img, 32, 20), 0xE87C8E);
    assert_near(px(&img, 24, 37), 0x17120F); // eyes lifted by 3
    assert_near(px(&img, 5, 53), 0xE87C8E);  // an outer arm on the floor
    let red = render(octopus(OctopusColor::Coral), Expr::Alert, Pose { flush: 1.0, ..Pose::default() });
    assert_near(px(&red, 32, 20), FLUSH_RGB);
}

#[test]
fn the_cat_ignores_the_other_mascots_pose_fields() {
    let busy = Pose { flush: 1.0, hop_px: 0.0, waving: true, wave: 1.0, beat: 1.0, ..Pose::default() };
    let a = render(CatLook::default(), Expr::Idle, Pose::default());
    let b = render(CatLook::default(), Expr::Idle, busy);
    assert_eq!(a, b);
}

#[test]
fn every_mascot_paints_every_face_and_accessory() {
    for m in Mascot::ALL { for e in Expr::ALL { for acc in CatAccessory::ALL {
        let look = CatLook { mascot: *m, accessory: *acc, ..CatLook::default() };
        render(look, e, Pose::default()); // no panic, no D2D error (EndDraw Ok)
    }}}
}
```
(`assert_near` allows ±6 per channel for anti-aliasing; pick sample points away from edges.)
- [ ] **Step 2: Run** `cargo test --locked mascot::` — fails.
- [ ] **Step 3: Implement**; `render.rs` halo uses `mascot_box`.
- [ ] **Step 4: Run** `cargo test --locked` — green.
- [ ] **Step 5: Commit** `feat(mascot): paint the falk and the sitting octopus`

---

### Task 7: The clamp tests for all three mascots

**Files:**
- Modify: `src/ui/geometry.rs` tests (`the_looking_cat_stays_in_the_idle_island`, `the_dancing_cat_stays_in_every_compact_shape`, `the_typing_cat_stays_in_the_idle_island`, `robot_and_content_share_every_compact_shape`, the notice badge check)
- Modify (only if a test fails): `src/ui/mascot.rs` geometry (crest feather lengths or a `CREST_MAX_SCALE`, the octopus wave arm end, the octopus nightcap) and the memory note text later

**Interfaces:**
- Consumes: `falk_outline`, `octopus_outline`, `accessory_corners(m, ..)`, `anchors(m).eye_lift`, the Pose fields.

- [ ] **Step 1: Write the tests**: in each listed test, loop `for m in Mascot::ALL` and use the mascot's outline instead of `head_outline` + `ear_drawn` for the falk and the octopus (points are grid units; transform with the existing `drawn`, marks with the eye lift added to `gy`). Add the moving extremes: the falk at `crest_pose` sway ±9 (dancing, `beat` π/2 and 3π/2), Alert at 1.22, the Wave crest at ±10; the octopus at the dance sway extremes (`beat` sampled every π/8), the wave arm at `wave` ±1 (it shows in notices: check it inside the notice shape with `head_inside`-style containment of the badge square's shape), the typing arms at paw lift 0 and 1.6; the hop adds `HOP_PX` up in notices. Every accessory (all ten) for every mascot.
- [ ] **Step 2: Run** `cargo test --locked geometry::` — read which mascot, size and point fail.
- [ ] **Step 3: Fix the geometry, not the island**: shrink the offending piece minimally (record each change and its reason in the commit message, e.g. "falk crest feathers 0.9 at 28 dip").
- [ ] **Step 4: Run** `cargo test --locked` — green.
- [ ] **Step 5: Commit** `test(mascot): every mascot stays in every island`

---

### Task 8: Settings → Maskot

**Files:**
- Modify: `src/i18n.rs` (keys), `src/ui/settings_layout.rs` (`cat_page`, `Hit`, `Control`, `State`), `src/ui/settings.rs` (`Action`, model, apply, save), `src/ui/settings_controls.rs` (painting the mascot tiles, coat swatches per mascot, accessory tiles with the chosen mascot)

**Interfaces:**
- Consumes: `CatLook`, `Mascot`, coats (Task 1), painter (Task 6).
- Produces:
  - `Hit::Mascot(Mascot)`, `Hit::FalkColor(FalkColor)`, `Hit::OctopusColor(OctopusColor)`; `Action::Mascot(Mascot)`, `Action::FalkColor(FalkColor)`, `Action::OctopusColor(OctopusColor)` that set the config fields and save like `CatColor`.
  - `Control::MascotTiles { items: Vec<(Mascot, Rect)>, selected: Mascot, enabled: bool }`; `Control::CatSwatches` generalised to `CoatSwatches { items: Vec<(Coat, Rect)>, selected: Coat, enabled: bool }` (or three variants; builder's call).
  - i18n: `SecCat` → "Maskot" / "Mascot"; `GroupCat` → "Maskot" / "Mascot"; `RobotInNotch` → "Maskot di notch" / "Mascot in the notch"; `RobotInNotchDesc` → "Tampilkan maskot Vale di samping jam dan notice" / "Show Vale's mascot beside the clock and notices"; new `MascotLabel` "Maskot" / "Mascot", `MascotCat` "Kucing" / "Cat", `MascotFalk` "Falk" / "Cockatiel", `MascotOctopus` "Gurita" / "Octopus", coat names "Lutino", "Abu-abu" / "Grey", "Muka putih" / "White face", "Koral" / "Coral", "Merah" / "Red", "Ungu" / "Purple", "Biru laut" / "Sea blue", accessory names "Headphone", "Topi pelaut" / "Sailor cap", "Dasi kupu-kupu" / "Bow tie". Section search aliases keep "cat" / "kucing" and add "maskot" / "mascot". Other labels that say "kucing" about the notch mascot (list them with `rg -i kucing src/i18n.rs`) say "maskot"; the sticky-note "Kucing ngintip" stays (that peek stays the cat).
  - Layout: a new first row in the card, "Maskot" with three tiles (each `ACC_TILE` square, the mascot's idle face still in its default-or-chosen coat on its Auto surface), the card one row taller; the coat row shows the chosen mascot's coats (3, 4 or 4 swatches; falk swatches show the face colour, octopus the coat colour); the accessory tiles wrap into rows of at most 5 when 10 do not fit the content width (`ACC_ROW_H` grows by `ACC_TILE + 12` per extra row); the preview and tiles draw the chosen mascot.

- [ ] **Step 1: Failing tests** (settings_layout / settings tests, following the existing Kucing page tests):

```rust
#[test]
fn the_maskot_page_offers_three_mascots_and_their_coats() {
    for m in Mascot::ALL {
        let l = layout_for(State { mascot: *m, ..State::default() }, Section::Cat, 720.0, 520.0);
        assert_eq!(mascot_tiles(&l).len(), 3);
        let want = match m { Mascot::Cat => 4, Mascot::Falk => 3, Mascot::Octopus => 4 };
        assert_eq!(coat_swatches(&l).len(), want, "{m:?}");
        assert_eq!(accessory_tiles(&l).len(), 10);
        assert_no_overlap(&l.controls); assert_inside_cards(&l);
    }
}

#[test]
fn picking_a_mascot_keeps_each_coat() {
    let mut m = Model::default();
    m.apply(Action::Mascot(Mascot::Falk)); m.apply(Action::FalkColor(FalkColor::Grey));
    m.apply(Action::Mascot(Mascot::Cat));
    assert_eq!((m.cat_color, m.falk_color, m.mascot), (CatColor::Terracotta, FalkColor::Grey, Mascot::Cat));
}

#[test]
fn the_labels_say_maskot() {
    assert_eq!(text(Lang::Id, Key::SecCat), "Maskot");
    assert_eq!(text(Lang::Id, Key::RobotInNotch), "Maskot di notch");
    assert_eq!(text(Lang::En, Key::MascotFalk), "Cockatiel");
}
```
(Use the module's real helper names for building a layout and reading controls; the assertions are what matter.)
- [ ] **Step 2: Run** `cargo test --locked settings` — fails.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --locked` — green; then `cargo run --release -- --settings` is a manual look only if the session can show a window (else skip and list it as a hand check).
- [ ] **Step 5: Commit** `feat(mascot): Settings Maskot page`

---

### Task 9: Demo scenes

**Files:**
- Modify: `src/ui/demo.rs` (env parsing ~2271, the scene list, the face sheet in `render.rs::paint_robot_sheet` if it builds its own look)

**Interfaces:**
- Consumes: `CatLook`, `Mascot`, coats, Pose fields.
- Produces: env `DI_MASCOT` (`cat`, `falk`, `octopus`), `DI_FALK_COLOR` (`lutino`, `grey`, `whiteface`), `DI_OCTOPUS_COLOR` (`coral`, `red`, `purple`, `blue`); every scene's look from them; new `--demo-robot` scenes `maskot-falk` (the eleven faces), `maskot-gurita` (the eleven faces), `maskot-warna` (every coat of the three on its Auto surface), `maskot-ekstra` (falk hop at its top, octopus flush 1.0, octopus asleep coiled, both waving at `wave` 1). Docs in the module header like `DI_CAT_COLOR`.

- [ ] **Step 1: Failing test**: `demo_look_reads_the_mascot_env` — with `DI_MASCOT=falk` and `DI_FALK_COLOR=grey` the parsed look is `CatLook { mascot: Falk, falk: Grey, .. }`; unknown values give the defaults (test the pure parse fn, not the env).
- [ ] **Step 2: Run** — fails. **Step 3: Implement.** **Step 4: Run** `cargo test --locked` — green; `cargo build --release --locked` succeeds.
- [ ] **Step 5: Commit** `feat(mascot): demo scenes for the falk and the octopus`

---

## After the tasks (finishing, per AGENTS.md)

Whole-branch review; merge latest `main` into the branch, then merge locally into `main`; `cargo test --locked` on `main`; exe size before/after into [[exe-size]]; memory: a new note `docs/memory/maskot-falk-gurita.md` (decisions, the geometry tweaks from Task 7, what is not verified live) linked from the index, updates to `cat-accessories`, `cat-colours-and-surfaces`, `settings-structure`, `bata-idle-power-budget`, `bata-robot-expressions`; README's mascot mention; commit "docs: update memory and README"; push `main`; remove the worktree and branch. Hand checks for the owner: each mascot in the real notch at 28 and 44 dip, solid and see-through, every coat, the new accessories, dance with real music, typing while Claude runs, a greeting, a reminder alert, sleep, Settings switching back and forth.
