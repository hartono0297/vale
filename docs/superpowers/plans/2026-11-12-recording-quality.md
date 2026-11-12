# Screen recording quality choice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A "Kualitas video" choice (Asli · 1440p · 1080p · 720p) in Settings → Fitur → Rekam layar that caps the screen recording's size, with a per-minute size estimate as the row's description.

**Architecture:** The pure size rule in `src/screenrec/plan.rs` (`output_size`) already takes an optional cap; it becomes a cap on the shorter side and gains a size estimate. A `VideoQuality` enum lives with the other recording prefs (`src/recorder/prefs.rs`), flows into the start job and `session::options_for`, and Settings shows it as a segmented row like 30/60 fps.

**Tech Stack:** Rust 2024, Win32 / Direct2D Settings window (own layout engine in `src/ui/settings_layout.rs`), Media Foundation encoder (unchanged).

**Spec:** `docs/superpowers/specs/2026-11-12-recording-quality-design.md`

## Global Constraints

- Choices, in order: Asli, 1440p, 1080p, 720p (English: Original, 1440p, 1080p, 720p). Default Asli.
- Config key `recording.video_quality`, values `"original"`, `"1440p"`, `"1080p"`, `"720p"`; anything else or missing reads as `"original"`; the rest of the section and file always load.
- A choice caps the **shorter** side (1440, 1080, 720), aspect kept, never scaled up; the encoder caps `MAX_W` 4096 × `MAX_H` 2304 and `MIN_SIDE` 64 still apply; both sides even.
- Estimate text: Indonesian `{w}×{h} · ± {n} MB/menit`, English `{w}×{h} · ± {n} MB/min` (`×` is U+00D7, `·` U+00B7, `±` U+00B1). `n` = (video bitrate from `plan::bitrate` + 96,000 bit/s) × 60 / 8 / 1,048,576, rounded to nearest, at least 1.
- The estimate's source size is the primary monitor from Settings' monitor list (`MonitorRow.primary`), else the first row, else 1920×1080.
- Choice is refused while a screen recording is busy (same as `screen_set_fps`), and a recording keeps the quality it started with.
- Settings text must fit its row in both languages at the 720×520 minimum window.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Run `cargo test --locked` (all tests) before each commit. No Fable model anywhere.

## Review Focus

- A portrait monitor or a tall dragged area at 1080p must come out 1080 wide, not 1080 tall (shorter side), e.g. 1440×2560 → 1080×1920 — Task 1 test.
- A small window (shorter side under the cap) must keep its own size, never be scaled up — Task 1 test.
- An old config.json without `video_quality`, or with a bad value (`"4k"`, `1080`, `null`), must load with Asli and keep every other recording setting — Task 2 test.
- Changing quality while a recording runs must not change that recording (start job snapshot) and must be refused like fps — Task 2 (handler mirrors `screen_set_fps`) and its review.
- The longest estimate line (`4096×2304 · ± 325 MB/menit` at 60 fps; English "Original" is the widest segment label) must not run under the segmented control — Task 3 layout test.

---

### Task 1: Shorter-side cap and the size estimate

**Files:**
- Modify: `src/screenrec/plan.rs` (`output_size`, new `estimate_mib_per_minute`, tests)
- Modify: `src/screenrec/mod.rs:116-118, 140, 438` (rename `Options::max_height` → `max_short`)
- Modify: `src/screenrec/session.rs:26, 152` (follow the rename only; signature change is Task 2)
- Modify: `src/screenrec/debug.rs` (`--quality <original|1440p|1080p|720p>` flag for `--screenrec-test`)

**Interfaces:**
- Produces: `pub fn output_size(w: u32, h: u32, max_short: Option<u32>) -> (u32, u32)` — `max_short` caps `min(w, h)`.
- Produces: `pub fn estimate_mib_per_minute(w: u32, h: u32, fps: u32) -> u32` — for an already-planned output size.
- Produces: `pub const AUDIO_BITS_PER_SECOND: u32 = 96_000;` in `plan.rs` (doc: the mono AAC track, `encode.rs` uses 12,000 bytes/s).
- Produces: `Options { max_short: Option<u32>, .. }`.

- [ ] **Step 1: Write the failing tests** in `plan.rs`'s test module

```rust
#[test]
fn a_quality_caps_the_shorter_side_and_never_scales_up() {
    assert_eq!(output_size(2560, 1440, Some(1080)), (1920, 1080));
    assert_eq!(output_size(3440, 1440, Some(1080)), (2580, 1080));
    assert_eq!(output_size(1440, 2560, Some(1080)), (1080, 1920));
    assert_eq!(output_size(3840, 2160, Some(720)), (1280, 720));
    assert_eq!(output_size(2560, 1440, Some(1440)), (2560, 1440));
    assert_eq!(output_size(900, 600, Some(1080)), (900, 600));
    assert_eq!(output_size(1920, 400, Some(720)), (1920, 400));
    // The encoder's cap still wins over a looser quality.
    let (w, h) = output_size(7680, 4320, Some(2160));
    assert!(w <= MAX_W && h <= MAX_H);
}

#[test]
fn the_estimate_is_video_plus_audio_per_minute_in_mib() {
    assert_eq!(estimate_mib_per_minute(1920, 1080, 30), 36);
    assert_eq!(estimate_mib_per_minute(2560, 1440, 30), 64);
    assert_eq!(estimate_mib_per_minute(1280, 720, 30), 17);
    assert_eq!(estimate_mib_per_minute(1920, 1080, 60), 72);
    assert_eq!(estimate_mib_per_minute(2560, 1440, 60), 127);
    assert_eq!(estimate_mib_per_minute(4096, 2304, 60), 325);
    assert!(estimate_mib_per_minute(64, 64, 30) >= 1);
}
```

Keep the existing `Some(1080)` assertions (lines ~236-238): they still hold under the shorter-side rule.

- [ ] **Step 2: Run them, expect FAIL**

Run: `cargo test --locked screenrec::plan`
Expected: FAIL (`estimate_mib_per_minute` not found; `3440x1440` and portrait cases wrong).

- [ ] **Step 3: Implement**

`output_size`: the scale is `min(MAX_W / w, MAX_H / h, cap / min(w, h), 1.0)` where `cap = max_short.clamp(MIN_SIDE, MAX_H)`; the min-side rule and `even` stay as they are. `estimate_mib_per_minute`: `(bitrate(w, h, fps) as u64 + AUDIO_BITS_PER_SECOND as u64) * 60 / 8`, divided by 1,048,576 as f64, `.round()`, `.max(1)`. Update the module and function docs. Rename the `Options` field and its doc ("Scale the video down so its shorter side is at most this; `None` = the source's own size (capped at 4096 x 2304)"). In `debug.rs`, parse `--quality` into `max_short` (`original` → `None`, `1440p` → 1440, `1080p` → 1080, `720p` → 720, else an error "--quality needs original, 1440p, 1080p or 720p") and pass it in the `Options` built at `run`.

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test --locked` — all pass.

- [ ] **Step 5: Commit** — `feat(screenrec): cap the shorter side, estimate MB per minute`

---

### Task 2: The `video_quality` setting, into the recording

**Files:**
- Modify: `src/recorder/prefs.rs` (enum, field, read, write, tests, module doc)
- Modify: `src/screenrec/session.rs` (`options_for` signature, tests)
- Modify: `src/app_screenrec.rs` (`StartJob.quality`, the job built at ~785, `options_for` call at ~523, new `screen_set_quality`)
- Modify: `src/ui/settings.rs` (`Action::VideoQuality(VideoQuality)` next to `Action::VideoFps`, applied to `m.recording.video_quality` at ~4590)
- Modify: `src/app.rs:6562` (route `SettingsAction::VideoQuality(q) => self.screen_set_quality(q)`)
- Modify: `src/config.rs:~1372` (the full-config test fixture gets `video_quality`)

**Interfaces:**
- Consumes: `Options::max_short` (Task 1).
- Produces, in `src/recorder/prefs.rs`:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VideoQuality { #[default] Original, P1440, P1080, P720 }
impl VideoQuality {
    pub const ALL: [VideoQuality; 4]; // in the order above
    pub fn max_short(self) -> Option<u32>; // None, 1440, 1080, 720
    pub fn key(self) -> &'static str;     // "original", "1440p", "1080p", "720p"
    pub fn from_key(s: &str) -> Option<VideoQuality>;
}
```

  and `RecordingPrefs.video_quality: VideoQuality` (default `Original`).
- Produces: `pub fn options_for(fps: u32, sources: RecSources, title: &str, effects: CursorEffects, quality: VideoQuality) -> Options` setting `max_short: quality.max_short()`.
- Produces: `App::screen_set_quality(&mut self, q: VideoQuality)` — same shape as `screen_set_fps` (refused while `self.screen.busy()`, saves only on change, always `sync_settings()`).
- Produces: `settings::Action::VideoQuality(VideoQuality)` (the variant Task 3's click handler posts).

- [ ] **Step 1: Write the failing tests**

In `prefs.rs`:

```rust
#[test]
fn video_quality_reads_leniently_and_round_trips() {
    for (v, q) in [
        (json!("original"), VideoQuality::Original),
        (json!("1440p"), VideoQuality::P1440),
        (json!("1080p"), VideoQuality::P1080),
        (json!("720p"), VideoQuality::P720),
        (json!("4k"), VideoQuality::Original),
        (json!(1080), VideoQuality::Original),
        (json!(null), VideoQuality::Original),
    ] {
        let p = RecordingPrefs::from_json(&json!({"video_quality": v, "video_fps": 60}));
        assert_eq!(p.video_quality, q);
        assert_eq!(p.video_fps, 60, "the other keys still load");
    }
    assert_eq!(RecordingPrefs::from_json(&json!({})).video_quality, VideoQuality::Original);
    for q in VideoQuality::ALL {
        let p = RecordingPrefs { video_quality: q, ..RecordingPrefs::default() };
        let back: RecordingPrefs = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
    assert_eq!(VideoQuality::ALL.map(VideoQuality::max_short), [None, Some(1440), Some(1080), Some(720)]);
}
```

In `session.rs`, extend the existing `options_for` tests: `options_for(30, .., VideoQuality::Original).max_short == None`, `VideoQuality::P1080` → `Some(1080)`, `VideoQuality::P720` → `Some(720)`; existing calls pass `VideoQuality::Original`.

In `settings.rs`, beside the `Action::VideoFps` apply test (if one exists; else a new one): applying `Action::VideoQuality(VideoQuality::P720)` to a model sets `m.recording.video_quality`.

- [ ] **Step 2: Run, expect FAIL** — `cargo test --locked recorder::prefs screenrec::session` fails to compile (no `VideoQuality`).

- [ ] **Step 3: Implement** — the enum and field, `from_json` (`map.get("video_quality").and_then(Value::as_str).and_then(VideoQuality::from_key).unwrap_or_default()`), serialize as `key()` (bump `serialize_map(Some(11))`), the `options_for` parameter, `StartJob.quality: VideoQuality` filled from `recording.video_quality` where the job is built, the `screen_set_quality` handler, the `SettingsAction` route, the `config.rs` fixture.

- [ ] **Step 4: Run, expect PASS** — `cargo test --locked`, all pass.

- [ ] **Step 5: Commit** — `feat(screenrec): video_quality setting reaches the recorder`

---

### Task 3: The Settings row and its estimate

Load the antislop skills first (`antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting`) per the global rule; run their checklists on the row's copy and layout.

**Files:**
- Modify: `src/i18n.rs` (keys near `ScreenSettingsFps` at ~689; one formatter)
- Modify: `src/ui/settings_layout.rs` (`Hit::VideoQuality`, `Group::VideoQuality`, `VIDEO_QUALITY_CHOICES`, `screen_card` at ~4608, tests at ~9369)
- Modify: `src/ui/settings.rs` (click at ~384, segmented index at ~1915, group list at ~1942, the hit list at ~2586, click tests at ~4955)

**Interfaces:**
- Consumes: `VideoQuality` + `ALL` + `max_short` (Task 2), `plan::output_size`, `plan::estimate_mib_per_minute` (Task 1), `Action::VideoQuality` (Task 2).
- Produces: `Key::ScreenSettingsQuality` ("Kualitas video" / "Video quality"), `Key::ScreenSettingsQualityOriginal` ("Asli" / "Original"); `pub fn video_estimate_line(lang: Lang, w: u32, h: u32, mib: u32) -> String` in `i18n.rs` giving exactly `"1920×1080 · ± 36 MB/menit"` / `"1920×1080 · ± 36 MB/min"`.
- Produces: `Hit::VideoQuality(VideoQuality)`, `Group::VideoQuality`, `pub const VIDEO_QUALITY_CHOICES: [VideoQuality; 4] = VideoQuality::ALL;`, and `fn estimate_source(monitors: &[MonitorRow]) -> (u32, u32)` (primary, else first, else 1920×1080) in `settings_layout.rs`.

- [ ] **Step 1: Write the failing tests**

i18n: `video_estimate_line(Lang::Id, 1920, 1080, 36) == "1920×1080 · ± 36 MB/menit"` and the English one.

`settings_layout.rs`: rename and extend `the_screen_card_has_fps_the_two_switches_and_the_folder` to `the_screen_card_has_quality_fps_the_two_switches_and_the_folder`:
- the card is `5.0 * ROW_H`; the quality segmented sits in row 0, fps in row 1, highlight row 2, ripples row 3, folder row 4 (shift the existing row assertions by one);
- `l.segmented(Group::VideoQuality)` items' hits are `VIDEO_QUALITY_CHOICES.map(Hit::VideoQuality)` and labels `["Asli","1440p","1080p","720p"]` (Id) / `["Original","1440p","1080p","720p"]` (En); `selected` follows `s.recording.video_quality` for every choice;
- with `monitors` holding a primary 2560×1440 row (and a non-primary 1920×1080 row before it), `P1080` at 30 fps shows the text `"1920×1080 · ± 36 MB/menit"`; `Original` shows `"2560×1440 · ± 64 MB/menit"`; `Original` at 60 fps shows `"2560×1440 · ± 127 MB/menit"`; with an empty monitor list `Original` shows `"1920×1080 · ± 36 MB/menit"`;
- fit: for both languages, with a primary 4096×2304 monitor, `Original` at 60 fps (the longest line, `"4096×2304 · ± 325 MB/menit"`), the estimate text's rect and the title's rect end left of the quality segmented's first segment (`text.rect.right() <= first_segment.x`), and the segmented's right edge is inside the card. Use the layout's measured text rects the way the other fit tests in this file do (look for the "Setelah capture" description width test and copy its measuring approach).

`settings.rs` click test next to the fps one: clicking `Hit::VideoQuality(P720)` with `Original` set posts `[After::Post(Action::VideoQuality(P720))]`; clicking the selected one posts nothing.

Any other test that counts Fitur cards or heights (e.g. `exactly_the_overflowing_sections_get_a_viewport`, the `cards.len() - 4` lookups) must still pass; Fitur already scrolls, so only the card's index math may shift.

- [ ] **Step 2: Run, expect FAIL** — `cargo test --locked settings` (missing items).

- [ ] **Step 3: Implement** — keys and formatter; `Hit`/`Group` variants; in `screen_card` a 5-row card with the quality segmented first (labels from `Key::ScreenSettingsQualityOriginal` and `"1440p"`, `"1080p"`, `"720p"`), its description `video_estimate_line(lang, w, h, estimate_mib_per_minute(w, h, fps))` where `(w, h) = output_size(src.0, src.1, quality.max_short())` and `src = estimate_source(s.monitors)`; fps moves to row 1. Update the doc comment of `screen_card`. In `settings.rs`: the click (`Hit::VideoQuality(q) if q != prefs.video_quality => Post(Action::VideoQuality(q))`), the segmented index (`VIDEO_QUALITY_CHOICES` position), the group list and the hit list where `Hit::VideoFps(_)` appears.

- [ ] **Step 4: Run, expect PASS** — `cargo test --locked`, all pass; `cargo build --release --locked` with 0 warnings.

- [ ] **Step 5: Look at it** — run `target/release/vale.exe --settings fitur` (stop any running `vale` first), scroll to Rekam layar, screenshot in both languages; check nothing clips or overlaps and the estimate changes when 60 fps is picked.

- [ ] **Step 6: Commit** — `feat(settings): Kualitas video row with a size estimate`

---

## After the tasks (controller)

- Whole-branch review, then live check with the debug runner: `vale.exe --screenrec-test 5 --quality 720p` (and `1080p`, `original`) and confirm the frame size it prints (the runner reads `MF_MT_FRAME_SIZE`) is 1280×720 / 1920×1080 / 2560×1440 on the 2560×1440 primary monitor.
- Measure the exe size against `main` for `docs/memory/exe-size.md`.
- Hand checks for the owner: record a screen, a small window and an area at each choice and read the file's resolution in Explorer → Properties → Details; compare a 1-minute file's size with the estimate.
