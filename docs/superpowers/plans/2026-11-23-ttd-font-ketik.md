# Typed-signature font choice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The sign maker's Type tab offers four bundled handwriting fonts as "Aa" tiles; the preview and the made mark use the chosen one.

**Architecture:** Four OFL fonts are subset and renamed by `scripts/make-fonts.ps1`, embedded as RCDATA 211-214, loaded into their own DirectWrite in-memory collection (`ui::fonts::sign_collection`). A new `sign::hand` module names the fonts, builds their text formats (with the old installed-font chain as fallback) and remembers the last one used. `ink::from_text` takes the font; the maker gets a row of four tiles.

**Tech Stack:** Rust 2024, `windows` crate (Direct2D, DirectWrite), fontTools 4.67.0 via `make-fonts.ps1`.

**Spec:** `docs/superpowers/specs/2026-11-23-ttd-font-ketik-design.md`

## Global Constraints

- Fonts, pinned to google/fonts commit `06bfe0d62a98375cb09c942fa18e68bd32cff057` (`https://raw.githubusercontent.com/google/fonts/<commit>/ofl/<folder>/<file>`, `[`/`]` as `%5B`/`%5D`):

  | Output file | Family (name ID 1 and 16) | Source | Source SHA-256 |
  |---|---|---|---|
  | `SignSlant.ttf` | `Vale Sign Slant` | `dancingscript/DancingScript[wght].ttf`, instanced `{wght: 600}` | `21808625578FE8D8CD10CB684BE546DCA077B27CD03A53A2F1EC11DC743C924C` |
  | `SignThin.ttf` | `Vale Sign Fine` | `sacramento/Sacramento-Regular.ttf` | `9341FDA10ADBFEB7EFC94302B34507A3E227D7E7F5C432DF3F5AC8753FF73D24` |
  | `SignPrint.ttf` | `Vale Sign Print` | `kalam/Kalam-Regular.ttf` | `57CECB63D4608019371954274AE1D8C397764DEBD5B19D4A33C1EFA4DC923C0B` |
  | `SignFormal.ttf` | `Vale Sign Formal` | `greatvibes/GreatVibes-Regular.ttf` | `8D509802186F1B51572531ECF313E8098F9A5BFDFACA93F0C9B34467F9982D15` |

  Each folder's `OFL.txt` is pinned too (compute its SHA-256 on first download and write it into `$Sources`).
- Sign subset: unicodes `U+0020-007E,U+00A0-00FF,U+0100-017F,U+2010-2027,U+2030-203A,U+20AC,U+2122`, features `kern,liga,calt,ccmp,locl,mark,mkmk`, always `--no-hinting`, name IDs as `NAME_IDS`, `usWeightClass` 400, upright. Name records 1, 2, 3, 4, 6, 16, 17 must not contain "Dancing Script" or "Sacramento" (Reserved Font Names); name ID 0 (copyright) is kept as upstream wrote it.
- The eight existing files in `assets/fonts/*.ttf` must stay byte-identical after re-running `make-fonts.ps1`.
- RCDATA ids 211 (Slant), 212 (Thin), 213 (Print), 214 (Formal).
- No font names in the UI. Default font Slant; last-used font kept in a process-wide static, never on disk.
- Release exe stays under 6,373,068 bytes (Tanda tangan module cap).
- `cargo test --locked` green except the known drive-Z: test.
- Other sessions edit `src/sign/window/maker.rs` concurrently; keep changes there local to what each task names.

## Review Focus

1. Swash capitals and descenders (Great Vibes "K", "J"; Sacramento "g") reaching outside the bitmap: the typed mark must never be clipped. Task 3 pins it with `typed_marks_are_never_clipped`.
2. Names with Latin Extended-A letters ("Çağrı Żółć"): every font draws them from itself. Task 1 pins coverage counts, Task 3's clip test types them.
3. Bundled collection fails to load (resource missing): typing still gives a mark through the installed chain. Task 3 pins the chooser with `falls_back_to_installed_when_not_embedded`.
4. Tab and Shift+Tab around the name field: focus must not get stuck or skip the tiles. Task 2 pins the ring order; Task 4 pins the hand-off target.
5. Switching font or ink while a name is typed re-renders at once; switching tabs and back keeps the chosen font. Task 4 checks it by hand in `--demo-sign`.

## Parallel run

Task 1 (fonts, scripts, `app.rc`, notices, `src/ui/font_coverage.rs`) and Task 2 (`src/sign/window/maker.rs` layout only) touch disjoint files and run at the same time; Task 2 in its own worktree `.claude/worktrees/ttd-font-layout` on branch `ttd-font-layout` cut from this branch, merged back after review. Task 3 needs Task 1; Task 4 needs Tasks 2 and 3.

---

### Task 1: Build and embed the four sign fonts

**Files:**
- Modify: `scripts/make-fonts.ps1` (`$Sources`, `BuildPy`, `VerifyPy`, `$ofl`)
- Create: `assets/fonts/SignSlant.ttf`, `SignThin.ttf`, `SignPrint.ttf`, `SignFormal.ttf` (generated)
- Modify: `assets/fonts/OFL.txt` (generated), `app.rc`, `THIRD-PARTY-NOTICES.md`
- Test: `src/ui/font_coverage.rs`

**Interfaces:**
- Produces: RCDATA 211-214 holding the four TTFs; families `Vale Sign Slant`, `Vale Sign Fine`, `Vale Sign Print`, `Vale Sign Formal`, weight 400, style normal.

- [ ] **Step 1: Write the failing test** in `src/ui/font_coverage.rs`

```rust
const SIGN_FONTS: [(&str, &[u8], usize); 4] = [
    ("SignSlant", include_bytes!("../../assets/fonts/SignSlant.ttf"), SLANT_EXT_A),
    ("SignThin", include_bytes!("../../assets/fonts/SignThin.ttf"), THIN_EXT_A),
    ("SignPrint", include_bytes!("../../assets/fonts/SignPrint.ttf"), PRINT_EXT_A),
    ("SignFormal", include_bytes!("../../assets/fonts/SignFormal.ttf"), FORMAL_EXT_A),
];

#[test]
fn sign_fonts_keep_their_latin_subset() {
    for (name, bytes, ext_a) in SIGN_FONTS {
        let map = cmap(bytes);
        for cp in (0x20..=0x7E).chain(0xA0..=0xFF) {
            if cp != 0xAD { assert!(map.contains(&cp), "{name} lost U+{cp:04X}"); }
        }
        assert_eq!(map.range(0x100..=0x17F).count(), ext_a, "{name}: Latin Extended-A count changed");
        assert!(map.range(0x2190..=0x2193).next().is_none() && !map.contains(&0x2713), "{name}: UI-only symbols in a sign font");
    }
}
```

The `*_EXT_A` constants are the counts the built files have (expected about 127, 126, 85, 126); pin the real values after Step 3.

- [ ] **Step 2: Run it** — `cargo test --locked sign_fonts_keep` — expected: compile error, the files do not exist.

- [ ] **Step 3: Extend `make-fonts.ps1`** — add the four sources and four OFL texts to `$Sources`; in `BuildPy` add `SIGN_UNICODES`, `SIGN_FEATURES` and a `SIGN_JOBS` list `(source, location or None, output, family)` built with `set_names(font, family, 400)` and always `--no-hinting`; in `VerifyPy` add the four files to `EXPECT` (family, "Regular", "Regular", 400), keep `LIMIT` (370 KB) on the eight UI files only, add a separate 230 KB limit on the four sign files, skip the DM-specific checks for them, and fail if name IDs 1, 2, 3, 4, 6, 16, 17 contain "Dancing Script" or "Sacramento"; append four `$ofl` entries, titled e.g. `Dancing Script (The Dancing Script Project Authors), renamed Vale Sign Slant: SignSlant.ttf`.

- [ ] **Step 4: Run the script** — `powershell -File scripts/make-fonts.ps1` (fresh `%TEMP%` work dir). Expected: `verify ok`. Then `git status --short assets/fonts` shows only the four new files and `OFL.txt` modified; the eight existing `.ttf` are unchanged.

- [ ] **Step 5: Embed** — `app.rc`: `211 RCDATA "assets/fonts/SignSlant.ttf"` … `214 RCDATA "assets/fonts/SignFormal.ttf"`. `THIRD-PARTY-NOTICES.md` "Fonts": the four, each "based on <font> by <author>, © <copyright line>", the source repo link, SIL OFL 1.1, a line that the subsets are renamed (Vale Sign *) because a subset is a Modified Version and two of them carry Reserved Font Names; say they are used for typed signatures. Update the header comment of `make-fonts.ps1` and of `font_coverage.rs` to mention the sign fonts.

- [ ] **Step 6: Pin the counts and run** — `cargo test --locked font_coverage` — expected: all pass.

- [ ] **Step 7: Commit** — `feat(sign): bundle four handwriting fonts for typed signatures`.

---

### Task 2: Font tile row in the maker layout

**Files:**
- Modify: `src/sign/window/maker.rs` (`MHit`, constants, `maker_layout`, `ring`, `act`, `paint` placeholder, tests)

**Interfaces:**
- Produces: `MHit::Font(usize)` (0..4), `const FONT_ROW_H: f32 = 44.0`, four `MHit::Font(i)` buttons in the Type tab only; `fn ring_of(buttons: &[(MHit, Rect)]) -> Vec<MHit>` (what `ring` sorts, now pure; `ring(m)` calls it).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_type_tab_has_four_font_tiles_between_field_and_preview() {
    let t = maker_layout(1.0, Tab::Type, 0, false, &LAB);
    let f: Vec<Rect> = (0..4).map(|i| t.rect_of(MHit::Font(i)).expect("tile")).collect();
    assert!(t.rect_of(MHit::Font(4)).is_none());
    for r in &f {
        assert_eq!(r.h, FONT_ROW_H);
        assert!(r.y >= t.field.bottom() + 7.99 && r.bottom() <= t.pad.y - 7.99, "{r:?}");
    }
    assert!((f[0].x - t.field.x).abs() < 0.01 && (f[3].right() - t.field.right()).abs() < 0.01);
    for i in 1..4 {
        assert!((f[i].w - f[0].w).abs() < 0.01);
        assert!((f[i].x - f[i - 1].right() - 8.0).abs() < 0.01);
    }
    assert_eq!(t.pad.h, PAD_H - FIELD_H - 8.0, "the preview keeps its height");
    let d = maker_layout(1.0, Tab::Draw, 0, false, &LAB);
    assert!((t.size.1 - d.size.1 - (FONT_ROW_H + 8.0)).abs() < 0.01);
    assert_eq!(t.hit(f[2].x + f[2].w / 2.0, f[2].y + f[2].h / 2.0), Some(MHit::Font(2)));
    for tab in [Tab::Draw, Tab::Photo] {
        let l = maker_layout(1.0, tab, 2, true, &LAB);
        assert!(!l.buttons.iter().any(|(h, _)| matches!(h, MHit::Font(_))));
    }
}

#[test]
fn font_tiles_come_after_the_tabs_and_before_the_ink() {
    let t = maker_layout(1.0, Tab::Type, 1, false, &LAB);
    let order = ring_of(&t.buttons);
    let at = |h| order.iter().position(|x| *x == h).unwrap();
    assert!(at(MHit::Tab(Tab::Photo)) < at(MHit::Font(0)));
    assert_eq!([at(MHit::Font(1)), at(MHit::Font(2)), at(MHit::Font(3))], [at(MHit::Font(0)) + 1, at(MHit::Font(0)) + 2, at(MHit::Font(0)) + 3]);
    assert!(at(MHit::Font(3)) < at(MHit::Ink(true)));
}
```

The existing `the_maker_is_400_wide_and_holds_its_parts` must keep passing.

- [ ] **Step 2: Run them** — `cargo test --locked sign::window::maker` — expected: compile error (`MHit::Font`, `FONT_ROW_H`, `ring_of`).

- [ ] **Step 3: Implement** — Type tab area becomes `PAD_H + FONT_ROW_H + 8` high: field on top, the row 8 below it (four equal tiles across `area.w`, 8 apart), the preview pad 8 below the row with height `PAD_H - FIELD_H - 8`. `ring_of` sorts `Font(_)` with key 1 (same as Redo / PickPhoto, which the Type tab does not have). `act`: `MHit::Font(_) => {}` for now (Task 4 fills it). `paint`: no drawing for `Font` yet if a match needs an arm.

- [ ] **Step 4: Run** — `cargo test --locked sign::window::maker` — expected: PASS.

- [ ] **Step 5: Commit** — `feat(sign): font tile row in the maker's Type layout`.

---

### Task 3: `sign::hand` and typed marks in the chosen font

**Files:**
- Modify: `src/ui/fonts.rs` (add `SIGN_FONT_RESOURCE_IDS`, `sign_collection`)
- Create: `src/sign/hand.rs`; Modify: `src/sign/mod.rs` (`pub mod hand;`)
- Modify: `src/sign/ink.rs` (`from_text` signature, move `handwriting_family` out, margins, tests)
- Modify: `src/sign/window/maker.rs:512` only (`ink::from_text(&name, ink_of(m.blue), hand::last())`)

**Interfaces:**
- Consumes: RCDATA 211-214 and the family names from Task 1.
- Produces:
  - `pub const SIGN_FONT_RESOURCE_IDS: [u16; 4] = [211, 212, 213, 214];` and `pub fn sign_collection(dwrite: &IDWriteFactory) -> Option<&'static IDWriteFontCollection>` in `ui::fonts` (built once with the existing `build_collection` and in-memory loader, cached like `embedded`).
  - `sign::hand`: `pub enum HandFont { Slant, Thin, Print, Formal }` (`Clone, Copy, Debug, PartialEq, Eq`), `pub const ALL: [HandFont; 4]` in tile order, `pub fn family(self) -> &'static str`, `pub fn format(dwrite: &IDWriteFactory, font: HandFont, size: f32) -> Option<IDWriteTextFormat>` (weight normal, style normal, no wrap), `pub fn last() -> HandFont`, `pub fn set_last(f: HandFont)`, `#[cfg(test)] pub(crate) static TEST_LOCK: Mutex<()>`.
  - `ink::from_text(name: &str, ink: Ink, font: HandFont) -> Option<Mark>`; `Ink::color` becomes `pub(crate)`.

- [ ] **Step 1: Write the failing tests**

In `hand.rs`:
```rust
#[test] fn every_hand_font_loads_from_the_exe() // for each f in ALL: sign_collection(&dwrite) is Some and resolves f.family()
#[test] fn falls_back_to_installed_when_not_embedded() // pure chooser: choose(f, embedded = false, installed = |n| n == "Segoe Script") == Choice::Installed("Segoe Script"); choose(f, true, _) == Choice::Embedded(f.family()); choose(f, false, |_| false) == Choice::None
#[test] fn the_last_font_starts_slanted_and_round_trips() // takes TEST_LOCK; resets to Slant at the end
```
(`fn choose(font, embedded: bool, installed: impl Fn(&str) -> bool) -> Choice` is the pure selector `format` uses; installed order stays Ink Free, Segoe Script, Segoe Print.)

In `ink.rs` (replacing `typed_name_uses_installed_font`):
```rust
#[test] fn typed_name_makes_a_mark_in_every_font() // each f: from_text("Siti", Blue, f) is Some, w > 20, h > 10; "   " is None
#[test] fn each_font_gives_its_own_mark() // the four marks of "Crist" differ pairwise (size or pixels)
#[test] fn typed_marks_are_never_clipped() // for each f and each of "Kennedy Wurangian", "Çağrı Żółć", "Great Vibes", "Jj Qq Yy fg": the uncropped canvas has no pixel with alpha > 0 within 2 px of any edge
```
For the clip test, split `from_text` into `fn typed_canvas(name: &str, ink: Ink, font: HandFont) -> Option<(u32, u32, Vec<u8>)>` (premultiplied BGRA before `to_mark` / `crop`) and the crop.

- [ ] **Step 2: Run** — `cargo test --locked sign::hand sign::ink` — expected: compile errors.

- [ ] **Step 3: Implement** — margins large enough that the clip test passes for all four (start with `TEXT_PX` on each side and height `TEXT_PX * 3`; measure the layout's overhang with `GetOverhangMetrics` if a fixed margin is not enough). `format` uses `sign_collection` when the family resolves there, else the installed chain on the system collection. Check that `raster::offscreen`'s shared factory can use the collection (the test proves it).

- [ ] **Step 4: Run** — `cargo test --locked sign::` then `cargo test --locked` — expected: PASS (except the known drive-Z: test).

- [ ] **Step 5: Commit** — `feat(sign): typed signatures in four bundled handwriting fonts`.

---

### Task 4: Wire the tiles in the maker

**Files:**
- Modify: `src/sign/window/maker.rs` (`Maker.font`, `retype`, `act`, `on_key`, `WM_SIGN_EDIT`, `paint`, tests)

**Interfaces:**
- Consumes: Task 2's `MHit::Font(usize)` row; Task 3's `hand::{HandFont, ALL, format, last, set_last}`, `ink::from_text(name, ink, font)`, `Ink::color`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn picking_a_font_tile_sets_and_remembers_it() {
    let _l = hand::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(pick_font(2), Some(HandFont::Print));
    assert_eq!(hand::last(), HandFont::Print);
    assert_eq!(pick_font(9), None);
    assert_eq!(hand::last(), HandFont::Print, "a bad index changes nothing");
    hand::set_last(HandFont::Slant);
}
```

- [ ] **Step 2: Run** — expected: compile error (`pick_font`).

- [ ] **Step 3: Implement** — `fn pick_font(i: usize) -> Option<HandFont>` (`ALL.get(i)`, then `set_last`). `Maker` gets `font: HandFont` set from `hand::last()` at open; `act(MHit::Font(i))` sets it through `pick_font` and calls `retype`; `retype` passes `m.font`. Focus: `EDIT_TAB` moves to `MHit::Font(0)`; the "from the tabs into the name field" redirect in `on_key` now triggers when forward Tab lands on `MHit::Font(0)` (was `Ink(true)`). Paint each tile like the saved-mark tiles (same fill, border, hover), "Aa" centred in its font via `hand::format(&m.gfx.dwrite, f, 22.0 * ui)` in the current ink colour (`ink_of(m.blue).color()`); the chosen tile gets a 2 * ui accent border inside the tile (the focus ring stays the existing outer ring).

- [ ] **Step 4: Run** — `cargo test --locked` — expected: PASS (except drive Z:).

- [ ] **Step 5: Look at it** — `cargo run -- --demo-sign`, open the maker, Type tab: type "Crist", click each tile, switch ink, Tab through (tabs → name → four tiles → ink → keep → Cancel / Use), switch to Draw and back (font kept), Use. Take screenshots of the Type tab in dark and light for the report.

- [ ] **Step 6: Commit** — `feat(sign): pick a handwriting font in the Type tab`.

---

### Finish

- [ ] `cargo build --release --locked`; record `target/release/vale.exe` size against `main` (expect about +215 KB, must be under 6,373,068).
- [ ] Whole-branch review, merge latest `main` into this branch (resolve `maker.rs` conflicts with the Start over and colour sessions), `cargo test --locked`, then AGENTS.md steps 4-6.
