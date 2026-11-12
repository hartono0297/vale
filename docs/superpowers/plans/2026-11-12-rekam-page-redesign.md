# Rekam Page Redesign (direction B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the panel's Rekam page as direction B: a recorder card on the left (button, two source switches with their devices, live captions) and the tab's recordings on the right, grouped by day, with one shape for every transcript button.

**Architecture:** The page stays one pure layout function (`record_layout` in `src/ui/panel_layout.rs`, tested) feeding one painter (`paint_record` in `src/ui/render_panel.rs`) and the click regions (`regions`). The data it needs grows a little: each `RecEntry` carries its start time, the scan reports the total, and `RecordView` carries the missing-device flags, the quality label and the hotkey texts. Two new clicks: `Hit::RecQuality` (a `menu_pop` dropdown like the device menu) and `Hit::ScreenDiscardKeep`.

**Tech Stack:** Rust 2024, Direct2D / DirectWrite through the existing renderer, `chrono`.

**Spec:** `docs/superpowers/specs/2026-11-12-rekam-page-redesign-design.md`. Visual source: `docs/design/rekam-b.html` (open it in a browser; every state is drawn there at 560 × 290). Read both before any task.

## Global Constraints

- Windows 11 only; `cargo test --locked` stays green; tests never show windows (`docs/memory/test-spawns-no-window.md`).
- Panel stays 560 × 290 (`ShapeKind::Panel`); content box x `left(s)`..`right(s)`, y `content_top(s, strip)`..`content_bottom(s)`.
- No new timer, thread or poll; meters only while recording (`docs/memory/bata-idle-power-budget.md`).
- Colours from the island `Palette` only, plus `theme::REC_RED`, `theme::SOFT_RED`, `p.ochre`. Red only for record/stop/discard, ochre only for paused and a missing device.
- Every UI string through `src/i18n.rs` (Indonesian first, English second), no em dash. UI work loads `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-human`; copy work also `antislop:antislop-copywriting`.
- The closed REC notch (`record_compact_layout`, `paint_record_compact`, `notch-rec-mute`), Settings, the recorder, the transcript engine and the hotkeys do not change.
- Work in `.claude/worktrees/rekam-redesign` on branch `feat/rekam-redesign`. Never touch `AGENTS.md`.
- Parallel sessions: `rec-one-folder` also edits `scan_recent` in `src/app_record.rs`; `hover-tooltips` may add tooltips to the panel. Merge `main` into the branch before merging back and keep both sides.

## Review Focus

1. The waiting strip is up (24 dip shorter): nothing reaches past `content_bottom`, the card's top block drops to 68 and its label goes, the list fits fewer rows. Test in Task 3 (`the_strip_shrinks_the_top_block_and_nothing_passes_the_dots`).
2. Only one source on: its switch takes no click and the page can never ask for "no source". Test in Task 3 (`the_last_source_on_has_no_switch_click`).
3. Many recordings over several days: group labels eat height, the "more" line appears only when rows were cut and says total minus shown. Test in Task 3 (`the_more_line_counts_what_did_not_fit`).
4. Long device names, long video titles, "Menunggu model", JetBrains Mono: text ends in an ellipsis inside its rect, the pill grows to fit up to its max. Tests in Task 3 (`a_pill_is_at_least_76_and_grows_to_its_label`) and the visual check in Task 6.
5. A chosen device unplugged while idle: the device line is ochre and says so; the recording still falls back as today. Test in Task 5 (`a_chosen_device_that_is_gone_is_flagged_missing`).

---

### Task 1: Recording list data (start time, total, day label)

**Files:**
- Modify: `src/model.rs` (`RecEntry`, `Event::RecRecent`)
- Modify: `src/app_record.rs` (`scan_recent`, `entry_of`, `rec_rescan`, the `RecRecent` handler, the screen scan in `src/app_screenrec.rs` if it calls `scan_recent`)
- Modify: `src/i18n.rs` (`rec_day_label`)

**Interfaces:**
- Produces: `RecEntry.started_at: chrono::NaiveDateTime` (the start `title_of` already computes).
- Produces: `pub(super) struct RecScan { pub entries: Vec<RecEntry>, pub total: usize }`; `scan_recent(...) -> RecScan` (same parameters). `total` counts every listed file of that kind in the folder (`.part` included), the `skip` file excluded.
- Produces: `Event::RecRecent(Vec<RecEntry>, usize)` (entries, total); the app keeps the total beside the list (`self.rec.total` or the screen equivalent).
- Produces: `pub fn rec_day_label(lang: Lang, day: NaiveDate, today: NaiveDate) -> String`: "Hari ini" / "Today", "Kemarin" / "Yesterday", else `short_date(lang, day)` ("8 Nov").
- `RECENT_LIMIT` becomes 8 (the most rows the new list can show is 7 plus a spare).

- [ ] **Step 1: Write the failing tests**

In `src/app_record.rs` tests: `scan_reports_the_total_beyond_the_limit` (make 10 `.m4a` files in a temp dir with distinct mtimes, `scan_recent(.., limit 3, ..)` gives 3 entries and `total == 10`; with `skip` set to one of them, `total == 9`) and `an_entry_carries_its_start` (a sidecar with `started` 2026-11-08 01:08 local gives `started_at` equal to that).

In `src/i18n.rs` tests: `rec_day_label` for today, yesterday and 2026-11-07 in both languages: `"Hari ini"`, `"Kemarin"`, `"7 Nov"`, `"Today"`, `"Yesterday"`, `"7 Nov"`; plus `assert_no_leak` for each.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test --locked scan_reports_the_total an_entry_carries rec_day_label`
Expected: compile errors (`total`, `started_at`, `rec_day_label` missing).

- [ ] **Step 3: Implement**

`scan_recent` counts `files.len()` before `truncate`. Add the `Key`s for "Kemarin" / "Yesterday" only if no existing key says it (search `Kemarin` in `i18n.rs`; `rec_started` already prints it, reuse that key). Update every `Event::RecRecent` sender and match arm.

- [ ] **Step 4: Run the tests**

Run: `cargo test --locked`
Expected: all pass.

- [ ] **Step 5: Commit**

`git commit -m "feat(rekam): list entries carry their start; the scan reports the total"`

---

### Task 2: Copy for the new page

**Files:**
- Modify: `src/i18n.rs`

**Interfaces:**
- Produces keys (Indonesian, English):
  - `RecRecordLabel`: "Rekam", "Record"
  - `RecNow`: "Sekarang", "Now"
  - `RecMuted`: "Dibisukan", "Muted"
  - `ScreenDiscardAsk`: "Buang video ini?", "Throw this video away?"
  - `ScreenDiscardKeep`: "Simpan", "Keep"
  - `RecQuality`: "Kualitas", "Quality"
  - `ScreenIdleHint` text becomes "Layar, jendela atau area: dipilih setelah tekan Rekam", "Screen, window or area: you pick it after Record"
- Produces functions:
  - `pub fn rec_more(lang: Lang, n: usize) -> String`: "{n} lagi di folder" / "{n} more in the folder".
  - `pub fn rec_muted(lang: Lang, hotkey: Option<&str>) -> String`: "Dibisukan · Ctrl+Alt+M" / "Muted · Ctrl+Alt+M", or the bare word when `None`.
  - `pub fn rec_empty_hint(lang: Lang, hotkey: Option<&str>) -> String`: "Tekan Rekam, atau Ctrl+Alt+R dari mana aja" / "Press Record, or Ctrl+Alt+R from anywhere"; `None` gives "Tekan Rekam" / "Press Record".
  - `pub fn rec_quality_label(lang: Lang, q: VideoQuality, fps: u32) -> String`: "1080p · 30 fps"; `Asli` gives "Asli · 30 fps" / "Original · 30 fps" (reuse the Settings key for Asli).
- Retires (only if `grep` finds no other use): `ScreenDiscardSure`, `ScreenDiscard`, `Start` on this page, `TrPickFileShort`, `RecSourceBoth`. Keep any key the tray, notch or Settings still uses.

- [ ] **Step 1: Write the failing tests**

`rec_copy_reads_right_in_both_languages`: asserts each string above exactly, for both languages, `None` and `Some("Ctrl+Alt+M")` cases, `rec_more(Lang::Id, 9) == "9 lagi di folder"`, `rec_quality_label(Lang::En, VideoQuality::P1080, 60) == "1080p · 60 fps"` (use the real variant names from `recorder::prefs`). Every string goes through `assert_no_leak` and contains no `'\u{2014}'`.

- [ ] **Step 2: Run and see it fail**

Run: `cargo test --locked rec_copy_reads_right`
Expected: compile errors.

- [ ] **Step 3: Implement the keys and functions; remove retired keys that nothing uses.**

- [ ] **Step 4: Run** `cargo test --locked`. Expected: all pass (the key table tests included).

- [ ] **Step 5: Commit** `git commit -m "feat(rekam): copy for the redesigned page"`

---

### Task 3: The layout

**Files:**
- Modify: `src/ui/panel_layout.rs` (`RecordHead`, `RecordLayout`, `RecordLive`, `RecordRowLayout`, `record_layout`, the `PageLayout::Record` arm of `regions`, the `REC_*` page constants; the tests that use them)
- Modify: `src/model.rs` (`Hit::RecQuality`, `Hit::ScreenDiscardKeep`)

**Interfaces:**
- Consumes: nothing from Tasks 1-2 (the layout takes measured widths and flags).
- Produces:
  ```rust
  pub struct RecListInput<'a> {
      /// Row i starts a new day (true for row 0).
      pub new_day: &'a [bool],
      /// Measured pill label width per row; 0 = no pill (silent video).
      pub pills: &'a [f32],
      /// Every listed recording in the folder (`RecScan::total`).
      pub total: usize,
  }
  pub fn record_layout(
      s: ShapeRect, strip: bool, head: &RecordHead,
      live: Option<RecSources>, chosen: RecSources,
      list: &RecListInput, tab: RecKind, discard_armed: bool,
  ) -> RecordLayout
  ```
  `RecordHead` keeps `tabs_w`, drops the link widths and `discard_w`, adds `confirm_w: [f32; 2]` ("Buang", "Simpan" button labels).
  `RecordLayout` fields: `header, tabs, tabs_enabled, pick: Rect (22×22 icon), folder: Rect (22×22 icon), card: Rect, button: Rect, button_label: Option<Rect>, pause: Option<Rect>, discard: Option<Rect>, confirm: Option<RecConfirm>, clock: Option<Rect>, hint: Option<Rect>, quality: Option<Rect>, sources: Vec<RecSourceRow>, caption: Rect, caption_switch: Rect, list: Rect, groups: Vec<(usize, Rect)>, now: Option<Rect>, rows: Vec<RecordRowLayout>, more: Option<(usize, Rect)>, empty: Option<Rect>`.
  `pub struct RecConfirm { pub question: Rect, pub discard: Rect, pub keep: Rect }`.
  `pub struct RecSourceRow { pub source: Source, pub row: Rect, pub glyph: Rect /* icon idle, mute button live */, pub name: Rect, pub device: Rect, pub switch: Option<Rect> /* idle */, pub switch_to: Option<RecSources> /* None = locked */, pub meter: Option<Rect> /* live */ }`.
  `RecordRowLayout { row, well, time, title: Option<Rect>, length, pill }`.
- Geometry (dip): header row as today; `pick` then `folder` 22×22, 4 apart, ending `SETTINGS_GAP` left of the Settings button. Body top = header bottom + 8. Card 196 wide, radius 18 (paint), padding 10 × 12, gap 12 to the list. Top block 92 (68 with the strip): idle button 56 centred, label 16 under it (none with the strip); live pause 32, stop 52 (and discard 32 on a screen recording) in a row 12 apart, clock 20 high under them. Armed: question 18 high, buttons 26 high under it, 6 apart. Source rows 34 each (none on the idle Layar tab: `hint` 30 + `quality` 34 instead), caption row 34; glyph 16 idle / mute 24 live at the row's left, switch 28 × 16 at the right, meter 5 bars at the right while live. List: group label 16, row 28 (well 22), pill 22 high, width `clamp(label + 16, 76, 112)`; `now` row (28, under a today label) first while recording; `more` line 18; `empty` 32 high block.
- Hits (in `regions`): tabs when enabled; `RecPause`; `ScreenDiscard` on `discard` and on `confirm.discard`; `ScreenDiscardKeep` on `confirm.keep`; `RecToggle`; each source row's `glyph` → `RecMute(source)` while live; `device` → `RecDevice(source)`; `switch` → `RecSources(switch_to)` only when `Some`; `quality` → `RecQuality`; caption row → `RecCaptions`; `pick` → `RecPickFile`; `folder` and `more` → `RecFolder`; per row pill first → `RecTranscript(i)`, then row → `RecOpen(i)`; `now` → nothing.

- [ ] **Step 1: Write the failing tests** (replace the old Rekam layout tests that assert the three-way segment)

```rust
#[test] fn the_rekam_card_and_list_sit_inside_the_content_box()
// For tab in [Audio, Screen] × live in [None, Some(BOTH)] × strip in [false, true] × armed in [false, true] (armed only with Screen + live):
// every rect in the layout (card, button, sources' rects, caption, list, rows, groups, now, more, empty) is inside
// left(s)..right(s), content_top(s, strip)..content_bottom(s); card.w == 196.0; list.x == card.right() + 12.0.

#[test] fn the_last_source_on_has_no_switch_click()
// chosen = mic only, idle Audio: the mic row's switch_to == None; the system row's switch_to == Some(BOTH).
// chosen = BOTH: mic row switch_to == Some(system only), system row == Some(mic only).
// regions() holds no Hit::RecSources for a locked row.

#[test] fn the_more_line_counts_what_did_not_fit()
// new_day = [true, false, true, false, true, false, false, false], total = 30, idle Audio, no strip:
// rows.len() + groups take <= list height; more == Some((30 - rows.len(), _)).
// total == rows.len(): more == None.

#[test] fn a_pill_is_at_least_76_and_grows_to_its_label()
// pills = [20.0, 70.0, 120.0, 0.0] -> pill widths 76.0, 86.0, 112.0, 0.0.

#[test] fn the_strip_shrinks_the_top_block_and_nothing_passes_the_dots()
// strip = true, idle Audio: button_label == None; card.bottom() <= content_bottom(s); every row bottom <= list.bottom().

#[test] fn recording_puts_now_first_and_mutes_on_the_glyph()
// live = Some(BOTH): now.is_some(), now.y < rows[0].y; sources[i].switch == None, meter.is_some();
// regions() maps sources[0].glyph to Hit::RecMute(Source::Mic) and sources[0].device to Hit::RecDevice(Source::Mic).

#[test] fn an_armed_discard_asks_with_two_buttons()
// tab Screen, live Some(BOTH), armed: confirm.is_some(), pause == None, discard == None;
// regions has ScreenDiscard on confirm.discard and ScreenDiscardKeep on confirm.keep.
```

- [ ] **Step 2: Run** `cargo test --locked -- rekam the_last_source the_more_line a_pill_is the_strip_shrinks recording_puts an_armed_discard`. Expected: compile errors.

- [ ] **Step 3: Implement the new `record_layout`, structs and `regions` arm; add the two `Hit` variants (doc comments as the others). Keep `rows_that_fit`-style fitting by height; group labels count against it; reserve 18 for `more` only when not all fit.**

- [ ] **Step 4: Run** `cargo test --locked`. Expected: layout tests pass; `render_panel.rs` and `app.rs` may not compile yet. If so, finish Task 4's call-site changes minimally (pass the new arguments, paint nothing new) so the crate builds, and say so in the commit.

- [ ] **Step 5: Commit** `git commit -m "feat(rekam): direction B layout with source switches and day groups"`

---

### Task 4: Painting the page

**Files:**
- Modify: `src/ui/render.rs` (`RecordView`)
- Modify: `src/ui/render_panel.rs` (`record_head`, the `record_layout` call near line 848, `paint_record`, `rec_pill_look`, `rec_discard_look`, the layout-in-renderer test near line 7614)

**Interfaces:**
- Consumes: Task 3's `RecordLayout`, `RecListInput`; Task 2's copy functions; Task 1's `started_at` and total.
- Produces: `RecordView` gains `total: usize`, `missing: [bool; 2]` ([mic, system]: chosen device not found), `quality: String` (Task 2's label), `mute_hotkey: Option<String>`, `start_hotkey: Option<String>` (Ctrl+Alt+R on Audio, the screen one on Layar), `today: NaiveDate`.
- Produces: `fn rec_pill_look(p: &Palette, t: RowTranscript) -> PillLook` with `PillLook { fill: Option<u32>, border: Option<u32>, ink: u32, alpha: f32, progress: Option<f32> }`:
  None → fill `track`, ink `text`; Done → fill `track`, border `accent`; Running(n) → fill `raised`, border `track`, progress `n/100` in `raised_hi`; Queued / Waiting → border `track`, ink `secondary`; Failed → fill `track`, ink `SOFT_RED`; Off → as None with alpha 0.45.

- [ ] **Step 1: Write the failing test** `the_pill_looks_follow_the_spec` asserting each `RowTranscript` case above on the Espresso palette, and `the_pill_ink_holds_contrast` (each ink against its fill, or against `surface` when the fill is `None`, ≥ 4.5 by the repo's contrast helper; search `fn contrast` in `src/theme.rs`) for every `Surface`.

- [ ] **Step 2: Run and see it fail.**

- [ ] **Step 3: Implement `paint_record` after `docs/design/rekam-b.html`.** Card fill `raised`, radius `theme::radius::TILE` (18). Record ring stroke 3 in `track`, red dot 22 / stop square 18. Pause, resume and trash as `track` circles with `text` icons (`Icon::Pause`, `Icon::Play`, a trash icon from `ui/icons.rs`; add one if missing, in the existing icon style). Clock 15 dip tabular with a red dot (ochre when paused). Source rows: `track` hairline on top; idle glyph accent when on, the crossed icon in `secondary` when off; name `text` (secondary when off); device line 10 dip `secondary` (ochre with the missing text when `missing`), ellipsis, a 10 dip chevron after it; switch via `paint_switch`, at 0.45 alpha when `switch_to` is `None`. Live: mute button 24 `track` fill (outline and crossed icon when muted, no red), meter with `rec_meter` in `accent`, resting when muted or paused, device line replaced by `rec_muted(lang, mute_hotkey)` when muted. Caption row: `Icon` for CC (add `Icon::Captions` if missing), label, switch. Layar idle: hint centred 10.5 dip `secondary`, quality row with label, value `secondary`, chevron. List: group labels 10.5 dip 500 `secondary`; row well `raised` with the kind icon; time 12.5 dip 500 `text`; title (screen) 11.5 dip ellipsis; length `secondary` (`rec_duration` or "belum selesai"); pill by `rec_pill_look`, radius 8 (`radius::SEGMENT`), label 11 dip 500. `now` row: red / ochre dot in the well, `RecNow`, state word. `more`: `rec_more` + chevron in `secondary`. Empty: 32 well, `RecEmpty` / `ScreenNoVideos`, `rec_empty_hint`. Confirm: question 11.5 dip 500, Buang with `mix(raised, REC_RED, 0.16)` fill and `SOFT_RED` ink, Simpan `track`. Header icon buttons: `Icon::FileText`-style and `Icon::Folder` in `secondary`, hover fill `raised` and `text` icon (use the hover the Settings button already reads).

- [ ] **Step 4: Run** `cargo test --locked` and `cargo build --release --locked`. Expected: pass, no warnings added.

- [ ] **Step 5: Commit** `git commit -m "feat(rekam): paint the redesigned page"`

---

### Task 5: Wiring (missing devices, quality menu, keep, hotkey texts)

**Files:**
- Modify: `src/app_record.rs` (`rec_read_devices` → also sets `self.rec.missing`; a pure `fn device_missing(chosen: Option<&str>, found: bool) -> bool`)
- Modify: `src/app.rs` (`RecordView { .. }` near line 6965; `Effect` for the new hits; `WM_APP_MENU_DONE` also reads the quality menu)
- Create: `src/ui/quality_menu.rs` (like `src/ui/device_menu.rs`: `open(hwnd, anchor, Align::Start, current: (VideoQuality, u32), lang, font) -> bool`, `picked(&Done) -> Option<QualityPick>`, `pub enum QualityPick { Quality(VideoQuality), Fps(u32) }`)
- Modify: `src/state.rs` (`Hit::RecQuality` → `Effect::RecQualityMenu`; `Hit::ScreenDiscardKeep` → disarm, no effect besides what the current disarm does)
- Modify: `src/ui/mod.rs` (module)

**Interfaces:**
- Consumes: Task 3's hits; Task 4's `RecordView` fields.
- Produces: the quality menu rows: the four `VideoQuality` (Settings labels), a separator, "30 fps", "60 fps"; the current two checked. A pick saves through `save_config_with` exactly as Settings' `Action::VideoQuality` / `Action::VideoFps` do, refused while a recording is busy (same check as Settings).

- [ ] **Step 1: Write the failing tests**
  - `a_chosen_device_that_is_gone_is_flagged_missing`: `device_missing(Some("id"), false) == true`, `device_missing(Some("id"), true) == false`, `device_missing(None, false) == false`.
  - `quality_menu_rows_and_picks_map_back` in `quality_menu.rs`: the row list for `(P1080, 30)` has 7 rows with the right two checked; `picked` on each item id gives the matching `QualityPick`.
  - `keep_disarms_the_discard` in `state.rs`: arm with `Hit::ScreenDiscard` during a screen recording, then `Hit::ScreenDiscardKeep` leaves it disarmed and emits no discard effect.
  - `the_quality_row_opens_its_menu`: `Hit::RecQuality` on the idle Layar tab gives `Effect::RecQualityMenu`.

- [ ] **Step 2: Run and see them fail.**

- [ ] **Step 3: Implement.** Open the quality menu outside the app borrow, as `open_rec_device_menu` does (post a `WM_APP_*` message; claim a free `WM_APP + n` in the shared notes' slot table first). Fill `RecordView.mute_hotkey` from `config.hotkeys` (the mute row) and `start_hotkey` from the record or screen row, as their display text; `None` when unbound.

- [ ] **Step 4: Run** `cargo test --locked`. Expected: all pass.

- [ ] **Step 5: Commit** `git commit -m "feat(rekam): quality menu, keep button, missing-device line"`

---

### Task 6: Demo scenes and the visual check

**Files:**
- Modify: `src/ui/demo.rs` (the `rec-page*` scenes and their env vars, the module doc at the top)

**Interfaces:**
- Consumes: everything above.
- Produces env vars (documented in the module doc): `DI_REC_SOURCES=mic|system` (idle choice), `DI_REC_MISSING=mic|system`, `DI_REC_TR=all` (the six transcript states of `docs/design/rekam-b.html`'s transcript scene), `DI_REC_PAUSED=1`, `DI_REC_CAPTIONS=on` (exists), `DI_REC_TAB` / `DI_REC_LIVE` (exist), `DI_REC_MENU` (exists). Demo rows keep made-up data as today.

- [ ] **Step 1: Add the env vars and make every mockup state reachable.**

- [ ] **Step 2: Capture each state** with `docs/memory/screenshot-capture-script.md`'s method in Espresso and Terminal, Indonesian and English, DM Sans and JetBrains Mono (at least: idle both, mic only, device menu, missing, captions on, empty, recording, muted, paused, transcript states, transcripts off, Layar idle, Layar recording, armed, Layar empty).

- [ ] **Step 3: Compare with `docs/design/rekam-b.html`.** Nothing clipped or overlapping, text ellipsised inside its rect, colours as the spec. Fix what differs, re-run `cargo test --locked`.

- [ ] **Step 4: Commit** `git commit -m "feat(rekam): demo scenes for every page state"` and put the screenshots in the report (not the repo).
