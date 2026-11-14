# Shelf Image Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Image cards on the shelf get a small thumbnail, and right-click on one gives "Pratinjau gambar", which shows the image inside the panel.

**Architecture:** A WIC decoder returns BGRA pixels (`clipboard::image::Thumb`) on one short-lived worker thread; pure modules hold the thumbnail cache and the preview state; the panel renderer turns pixels into D2D bitmaps once per target (the `clip_thumbs` pattern). The preview is a sub-view of the Shelf page (like the phone link's views), not a window.

**Tech Stack:** Rust 2024, `windows` crate (WIC, Direct2D), existing Vale modules. No new crate, no new `windows` feature, no new timer.

**Spec:** `docs/superpowers/specs/2026-11-14-shelf-image-preview-design.md`

## Global Constraints

- No timer, and no thread, while the Shelf page is closed; no decode or file IO on the UI thread (spec anti-goals).
- Card thumbnail: long side at most 192 px. Preview: long side at most 1024 px. Never enlarged. Files over `phone::thumb::MAX_BYTES` (64 MiB) are not decoded.
- Cache cap: 12 entries (`shelf::MAX_ITEMS`). Network paths (`shelf::is_network`) get no card thumbnail; a preview of one is allowed (worker only).
- A failed decode (HEIC/AVIF without codec, corrupt, too big) is remembered as failed: never retried while the card stays.
- Text in both languages in `src/i18n.rs`. Keys: `ShelfPreview` "Pratinjau gambar" / "Preview image", `ShelfPreviewFailed` "Tidak bisa menampilkan gambar" / "Can't show this image", `ShelfPreviewLoading` "Memuat…" / "Loading…".
- Tests run with `cargo test --locked`. One test always fails on this PC because of drive Z: (not from the code); report it as pre-existing.
- Files shared with other sessions (`src/i18n.rs`, `src/app.rs` right-click code, `src/ui/render_panel.rs`): keep each edit small and local.

## Review Focus

- A card removed or turned stale while its decode is in flight: the late result is ignored, nothing panics, no cache entry for a card that is gone (Task 2, Task 3).
- A HEIC without the codec, a corrupt file and a zero-byte file: `Failed` once, the card keeps its icon, the preview shows the failed text, no retry on every `shelf_look` (Task 1, Task 2).
- Twelve large photos on page open: one worker, decoded one after another at 192 px, results arrive one by one, memory stays at 12 small bitmaps (Task 2, Task 3).
- Panel collapse, page change or the card leaving the shelf while the preview is open: the preview closes and the 1024 px pixels are dropped (Task 2, Task 6).
- Right-click on a non-image, stale or folder card, or on empty space: unchanged (tray menu) (Task 6).

---

### Task 1: Decode an image to BGRA pixels

**Files:**
- Create: `src/shelf_thumb.rs`
- Modify: `src/phone/thumb.rs` (share the WIC front half), `src/main.rs` (add `mod shelf_thumb;` beside `mod shelf;`)
- Test: inside `src/shelf_thumb.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `pub const CARD_SIDE: u32 = 192;` `pub const PREVIEW_SIDE: u32 = 1024;` `pub fn decode(path: &Path, max_side: u32) -> Option<Thumb>` (returns `crate::clipboard::image::Thumb`, top-down, alpha 255 on every pixel).
- Produces (in `phone/thumb.rs`, `pub(crate)`): `fn scaled_to(w: u32, h: u32, max_side: u32) -> (u32, u32)` (the body of today's `scaled_size`, which then calls it with `MAX_SIDE`) and `unsafe fn oriented_source(wic: &IWICImagingFactory, file: &File) -> Option<IWICBitmapSource>` (decoder from the file handle, frame 0, EXIF rotation applied: today's first half of `decode_and_encode`, which then calls it).

- [ ] **Step 1: Write the failing tests** in `src/shelf_thumb.rs` (use `phone::thumb::tests::write_gradient_png` and `phone::files::tests::temp_dir`, both already `pub(crate)`):
  - `a_big_png_is_scaled_to_the_card_side`: 640x480 → `decode(path, CARD_SIDE)` gives `(w, h) == (192, 144)`, `bgra.len() == 192*144*4`, every 4th byte (alpha) is 255.
  - `the_preview_side_scales_a_big_image_and_never_enlarges_a_small_one`: 2000x1500 at `PREVIEW_SIDE` → `(1024, 768)`; 50x40 → `(50, 40)`.
  - `a_file_that_is_not_an_image_is_none`: a text file named `a.png` and a zero-byte `empty.png` → `None`.
  - `a_file_over_the_cap_is_none_without_reading_it`: a sparse file made with `File::set_len(phone::thumb::MAX_BYTES + 1)` → `None`.
  - `a_missing_path_or_a_folder_is_none`.
  - In `phone/thumb.rs` add `scaled_to_uses_the_given_side`: `scaled_to(4000, 3000, 1024) == (1024, 768)` and `scaled_to(100, 50, 1024) == (100, 50)`.

- [ ] **Step 2: Run to verify they fail** — `cargo test --locked shelf_thumb` → compile error / FAIL (`decode` not defined).

- [ ] **Step 3: Implement.** Refactor `phone/thumb.rs` first (existing three tests there must still pass), then `decode`: open with `OpenOptionsExt::share_mode(7)` (read, write, delete), require `metadata().is_file()` and `len() <= MAX_BYTES` and `len() > 0`, join COM multithreaded like `thumb::make`, `oriented_source` → `CreateBitmapScaler` (Fant, `scaled_to`) → `CreateFormatConverter` to `GUID_WICPixelFormat32bppBGR` → `CopyPixels` into the `Vec`, then set every fourth byte to 255 (transparent areas come out dark; accepted).

- [ ] **Step 4: Run** — `cargo test --locked shelf_thumb phone::thumb` → PASS.

- [ ] **Step 5: Commit** — `git add src/shelf_thumb.rs src/phone/thumb.rs src/main.rs && git commit -m "feat(shelf): decode an image to BGRA pixels with WIC"`

### Task 2: The thumbnail cache and the preview state (pure)

**Files:**
- Create: `src/shelf_thumbs.rs`
- Modify: `src/shelf.rs` (add `previewable`), `src/main.rs` (`mod shelf_thumbs;`)
- Test: inside the new file

**Interfaces:**
- Consumes: `shelf::ShelfCard { key, kind, stale, .. }`, `shelf::ShelfKind`, `clipboard::image::Thumb`.
- Produces in `shelf.rs`: `pub fn previewable(card: &ShelfCard) -> bool` (kind `Image` and not `stale`).
- Produces in `shelf_thumbs.rs`:
  - `pub enum Slot { Pending, Ready(Arc<Thumb>), Failed }`
  - `pub struct ShelfThumbs` (`Default`) with `get(&self, key: &str) -> Option<&Slot>`; `wanted(&mut self, cards: &[ShelfCard], on_network: impl Fn(usize) -> bool) -> Vec<usize>` (indices into `cards` that are `previewable`, have no slot and are not on the network; marks them `Pending`); `set(&mut self, key: &str, thumb: Option<Arc<Thumb>>)` (only a `Pending` slot changes: `Some` → `Ready`, `None` → `Failed`; an unknown key is ignored); `retain(&mut self, cards: &[ShelfCard])` (drops slots whose key has no card).
  - `pub enum PreviewState { Loading, Failed, Ready(Arc<Thumb>) }`
  - `pub struct Preview { pub key: String, pub state: PreviewState }` with `Preview::new(key: String) -> Preview` (Loading), `set(&mut self, key: &str, thumb: Option<Arc<Thumb>>)` (ignored when `key` differs or state is not Loading) and `still_open(&self, cards: &[ShelfCard]) -> bool` (a previewable card with this key exists).
  - `pub fn fit_contain(w: f32, h: f32, bx: Rect) -> Rect` (the largest rect of the picture's ratio inside `bx`, centred, never above 1:1 scale) and `pub fn cover_source(w: f32, h: f32, box_w: f32, box_h: f32) -> (f32, f32, f32, f32)` (x, y, w, h of the source region that fills the box without distortion, centred). `Rect` is `ui::geometry::Rect`.

- [ ] **Step 1: Write the failing tests**:
  - `only_image_cards_that_are_there_are_previewable` (Image yes; Image stale no; Folder, Video, Document no).
  - `wanted_marks_pending_once_and_skips_network_and_non_images`: three cards (image, image on network by the closure, document) → `[0]`; a second call → `[]`.
  - `a_result_fills_a_pending_slot_a_failure_is_remembered_and_never_asked_again` (`set(.., None)` → `Failed`; `wanted` after → `[]`).
  - `a_result_for_a_card_that_left_is_ignored`: `retain` with no cards, then `set` → `get` is `None`.
  - `retain_keeps_only_cards_on_the_shelf`.
  - `preview_takes_only_its_own_result_once` and `preview_closes_when_its_card_goes_or_goes_stale`.
  - `fit_contain_keeps_the_ratio_centres_and_never_enlarges` (a 100x50 picture in a 300x300 box stays 100x50, centred; a 600x300 in a 300x300 box → 300x150 at y offset 75).
  - `cover_source_crops_the_long_side` (a 200x100 picture into a 50x50 box → source `(50, 0, 100, 100)`).

- [ ] **Step 2: Run to verify they fail** — `cargo test --locked shelf_thumbs` → FAIL.
- [ ] **Step 3: Implement** as specified; keep it free of Win32.
- [ ] **Step 4: Run** — `cargo test --locked shelf_thumbs shelf::` → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(shelf): thumbnail cache and preview state"` (add the three files).

### Task 3: Worker and App wiring for the thumbnails

**Files:**
- Modify: `src/app_shelf.rs`, `src/state.rs` (new `Event` variant, passed through like `ClipPacked` wherever that is listed), `src/app.rs` (route the event to the handler, like `Event::ClipPacked`)

**Interfaces:**
- Consumes: Task 1 `shelf_thumb::decode`, `CARD_SIDE`, `PREVIEW_SIDE`; Task 2 `ShelfThumbs`, `Preview`, `PreviewState`.
- Produces: `Event::ShelfDecoded { key: String, preview: bool, thumb: Option<Arc<Thumb>> }`; in `ShelfState` the fields `pub thumbs: ShelfThumbs` and `pub preview: Option<Preview>`; `App::shelf_decode(&mut self, jobs: Vec<(String, PathBuf, u32, bool)>)` (key, path, max side, is-preview; one `std::thread::Builder` named `shelf-thumb`, jobs in order, sends the event and calls the `poster(...)` wake per result like `clip_schedule_packing`, `thread::Builder` failure marks every job `set(.., None)`); `App::on_shelf_decoded(&mut self, key, preview, thumb)`.

- [ ] **Step 1:** There is no `App` unit-test harness; the pure rules are tested in Task 2. Add one test of the job building if you extract it as a pure function `shelf_jobs(cards, paths, wanted: Vec<usize>) -> Vec<(String, PathBuf, u32, bool)>` in `shelf_thumbs.rs`: `jobs_pair_each_wanted_index_with_its_path_and_the_card_side`.
- [ ] **Step 2: Run** — FAIL (not defined).
- [ ] **Step 3: Implement.** In `shelf_look` (end): `self.shelf.thumbs.retain(&self.shelf.cards)`; if the preview is open and `!still_open` drop it; if `self.state.view() == View::Panel(Page::Shelf)` call `wanted` (network test: `shelf::is_network(path)` of the card's path) and `shelf_decode` the jobs. `on_shelf_decoded`: `preview` true → `Preview::set`, else `ShelfThumbs::set`; redraw when the page is showing (use the same refresh call the phone progress path uses). Nothing runs while the page is closed: `sync_shelf` already calls `shelf_look` when the page opens.
- [ ] **Step 4: Run** — `cargo test --locked` (build + all) → PASS except the known Z: test.
- [ ] **Step 5: Commit** — `git commit -m "feat(shelf): decode thumbnails on a worker when the page is open"`

### Task 4: Thumbnails on the cards

**Files:**
- Modify: `src/ui/panel_layout.rs` (`ShelfCardLayout.thumb: Rect`, filled by `shelf_card_layout`), `src/ui/render.rs` (`Frame.shelf_thumbs: &'a ShelfThumbs`; the bitmap cache field and its clearing beside `clip_thumbs`), `src/ui/render_panel.rs` (`paint_shelf_card`), the place that builds `Frame` (follow `shelf: &...` in `app.rs`).

**Interfaces:**
- Consumes: `Slot`, `cover_source` (Task 2), `thumb_bitmap(&Thumb)` (existing).
- Produces: `ShelfCardLayout.thumb`: `rect(card.x + 4.0, card.y + 4.0, card.w - 8.0, label.y - 3.0 - (card.y + 4.0))`; `Renderer::shelf_thumb(&self, key: &str, thumb: &Thumb) -> Result<Option<ID2D1Bitmap>>` (cached per target in a `RefCell<Vec<(String, ID2D1Bitmap)>>`, at most 12, oldest dropped, cleared where `clip_thumbs` is cleared).

- [ ] **Step 1: Write the failing test** in `panel_layout.rs` tests: `a_shelf_cards_thumb_rect_sits_inside_the_card_above_its_label` (for one and two name lines, in a 3-card and a 12-card layout: `thumb` inside `card`, `thumb.bottom() < label.y`, width at least 40 dip). Existing shelf layout tests must pass untouched.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.** When the card's slot is `Ready` and the card is not stale: draw the bitmap cover-cropped into `lay.thumb` (source rect from `cover_source`, per-draw alpha only, no layer: the shelf motion rule) instead of the glyph; the name, X chip, "dari HP" tag and "hilang" chip are unchanged. Otherwise the icon as today.
- [ ] **Step 4: Run** — `cargo test --locked` → PASS (known Z: test aside).
- [ ] **Step 5: Commit** — `git commit -m "feat(shelf): thumbnails on image cards"`

### Task 5: The preview view

**Files:**
- Modify: `src/model.rs` or `src/state.rs` (wherever `Hit` and `Effect` live: add `Hit::ShelfPreviewBack`, `Hit::ShelfPreviewImage`, `Effect::ShelfPreviewClose`), `src/ui/panel_layout.rs` (`ShelfPreviewLayout`, `PageLayout::ShelfPreview`, `shelf_preview_layout`, the hit regions), `src/ui/render.rs` (`Frame.shelf_preview: Option<ShelfPreviewView<'a>>`), `src/ui/render_panel.rs` (`paint_shelf_preview`, the page choice at the `Page::Shelf =>` layout match), `src/ui/panel_tips.rs` (the new hit's tip, `Tip::named(Key::TipBack)` for the back button), `src/i18n.rs` (three keys), `src/app_shelf.rs` (handle `Effect::ShelfPreviewClose`: `self.shelf.preview = None`, redraw)

**Interfaces:**
- Consumes: `Preview`, `PreviewState`, `fit_contain` (Task 2); `phone_head(s, strip)` for the back button's place; `paint_phone_back` for the round back button.
- Produces: `pub struct ShelfPreviewLayout { pub back: Rect, pub title: Rect, pub image: Rect }`; `pub fn shelf_preview_layout(s: ShapeRect, strip: bool) -> ShelfPreviewLayout`; `pub struct ShelfPreviewView<'a> { pub name: &'a str, pub state: &'a PreviewState, pub key: &'a str }`. Reducer: both hits, only while `view() == View::Panel(Page::Shelf)`, push `Effect::ShelfPreviewClose` (no collapse).

- [ ] **Step 1: Write the failing tests**:
  - Layout: `the_preview_back_button_and_image_fit_the_panel` (for strip on/off and both shapes `back.right() < title.x`, `image` inside the panel content area and below the header, image at least 200 dip wide).
  - Layout regions: `the_preview_view_has_a_back_hit_and_an_image_hit` (the regions of `PageLayout::ShelfPreview` hold `Hit::ShelfPreviewBack` and `Hit::ShelfPreviewImage` and no `Hit::ShelfCard`).
  - Reducer (`state.rs` tests): `a_click_on_the_preview_back_or_image_asks_to_close_it_on_the_shelf_only` (on the Shelf page: `[Effect::ShelfPreviewClose]`; on another page: empty).
  - i18n: `the_preview_strings_exist_in_both_languages` (the three keys, both languages, non-empty).
  - Fit (render_panel tests, same style as `every_phone_string_holds_its_box_in_both_fonts`): `the_preview_strings_hold_their_boxes_in_both_fonts` for the title (a long name is cut with an ellipsis, never past `title`), the failed and loading text in `image`, both fonts, both languages.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.** The shelf page draws `paint_shelf_preview` instead of `paint_shelf` when `frame.shelf_preview` is `Some`; header is the back button + the file name (`heading`), the body is the image from the cached bitmap contained in `image` (cache `shelf_preview_bm: RefCell<Option<(String, ID2D1Bitmap)>>`, cleared with the target and replaced when the key changes), or the loading/failed text centred in secondary ink. Keep the existing 160 ms `Reveal` between views by following how `SubView.phone` does it; if that needs a reducer flag, add the smallest one and note it in the commit message.
- [ ] **Step 4: Run** — `cargo test --locked` → PASS (Z: aside).
- [ ] **Step 5: Commit** — `git commit -m "feat(shelf): the preview view in the panel"`

### Task 6: Right-click menu opens the preview

**Files:**
- Modify: `src/app.rs` (`RightClick::ShelfMenu(usize)`, `shelf_menu_target`, `on_right_click`, the `WM_RBUTTONUP` match, `open_shelf_card_menu`), `src/app_shelf.rs` (`shelf_preview_open`), `src/i18n.rs` already has the key from Task 5.

**Interfaces:**
- Consumes: `tray::note_card_menu(hwnd, label) -> bool` (a one-item menu; reuse as is), `shelf::previewable`, `Preview::new`, `App::shelf_decode`.
- Produces: `fn shelf_menu_target(view: View, hit: Option<&Hit>, cards: &[ShelfCard]) -> Option<usize>` (the index when the hit is `Hit::ShelfCard(i)` on `View::Panel(Page::Shelf)` and `cards[i]` is `previewable`); `App::shelf_preview_open(&mut self, i: usize)` (sets `shelf.preview = Some(Preview::new(key))` and queues one decode at `PREVIEW_SIDE` with `preview: true`; ignores a card that is no longer previewable; redraws).

- [ ] **Step 1: Write the failing tests** next to `note_card_menu_target`'s tests (or beside it): `a_right_click_on_an_image_card_gives_the_shelf_menu` and `a_right_click_on_a_stale_card_a_document_the_wrong_page_or_nothing_gives_no_shelf_menu`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.** In `on_right_click`, check `shelf_menu_target` after `clip_right_click` and before `note_card_menu_target`; the `WM_RBUTTONUP` arm calls `open_shelf_card_menu(hwnd, i)` which mirrors `open_note_card_menu` (`MENU_OPEN` guard, label `Key::ShelfPreview`, then `with_app(|app| app.shelf_preview_open(i))` if chosen). Everything else in the handler is untouched. A preview is dropped when the page is not the Shelf any more: in `sync_shelf` (`!wanted`) set `self.shelf.preview = None`.
- [ ] **Step 4: Run** — `cargo test --locked` → PASS (Z: aside).
- [ ] **Step 5: Commit** — `git commit -m "feat(shelf): right-click an image card to preview it"`

### Task 7: Demo scenes, docs, final check

**Files:**
- Modify: `src/ui/demo.rs` (demo scenes `shelf-thumbs` and `shelf-preview` with generated gradient pixels, no real files), `docs/memory/shelf.md` (link) and a new `docs/memory/shelf-image-preview.md` (decisions, gotchas, not verified live, exe size), `docs/memory/index.md` (one line), `docs/memory/exe-size.md` (the size now), `README.md` and `docs/guide.md` (one line about the right-click), `src/whats_new.rs` only if a release is asked (it is not).

- [ ] **Step 1:** Add the scenes and run `cargo run --release -- --demo` with `DI_DEMO_STEP=shelf-thumbs` and `shelf-preview` (debug builds: see the `shelf` scene for how scenes are picked); look at both in DM Sans and JetBrains Mono, Indonesian and English, Pill and Notch shape.
- [ ] **Step 2:** `cargo build --release --locked`, record `target/release/vale.exe` size against the last entry in `docs/memory/exe-size.md`.
- [ ] **Step 3:** `cargo test --locked` → everything passes except the known Z: test.
- [ ] **Step 4:** Write the memory note and the doc lines; list in the note what was not verified live: a real right-click on the island, thumbnails of real photos (HEIC from an iPhone), memory with twelve 20 MP photos.
- [ ] **Step 5: Commit** — `git commit -m "docs(shelf): image preview note, demo scenes"`
