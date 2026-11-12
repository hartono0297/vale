# Clipboard image drag-out Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An image card on the Clipboard page can be dragged out as a real PNG file onto any app or website that accepts files.

**Architecture:** A pure module `src/clipboard/drag.rs` owns the file name, the DIB-to-PNG pixel prep and the temp-file bookkeeping (`DragFiles`); a small Win32 part of the same module writes the PNG with WIC (the encoder in `clipboard/pack.rs`, generalised) and removes files. `app_clip.rs` tracks a press on an image card like `app_shelf.rs` tracks a shelf card, writes the file when the move passes 6 dip, and `wnd_proc` hands the path to `shelf_ole::drag_out` (now taking the allowed effects) outside the app borrow. One one-shot timer removes files 10 minutes after their last drag.

**Tech Stack:** Rust 2024, `windows` crate (OLE `DoDragDrop`, WIC PNG encoder, `SetTimer`), chrono for the local time in the name.

**Spec:** `docs/superpowers/specs/2026-11-12-clip-image-drag-design.md`

## Global Constraints

- Windows 11 x64 only; `cargo test --locked` must stay green (3874 tests on `main` at ad08324).
- Drag threshold `DRAG_START_DIP` = 6.0 dip times `gesture_scale()` (the shelf's).
- Temp root: `std::env::temp_dir().join("Vale").join("drag")`; one folder per card id: `<root>\<id>\<name>.png`.
- Name: `"{what} {YYYY-MM-DD} {HH.MM.SS}.png"`, `what` = `"Screenshot"` when `ClipImage::screenshot`, else `i18n::text(lang, Key::ClipKindImage)` ("Gambar" / "Image"); time = `Clip::at` in local time.
- `KEEP` = 600 s after the last drag of that card; `RETRY` = 60 s when a removal fails.
- Timer: `TIMER_CLIP_DRAG: usize = 55` (claimed in the sessions' merge protocol), one-shot only, set only while `DragFiles` is not empty. No repeating timer (`bata-idle-power-budget`).
- The clipboard drag offers `DROPEFFECT_COPY` only. The shelf keeps `COPY | LINK`.
- Never in tests or demos: no `win::read`/`win::write`, no real `%TEMP%` writes; file tests use `target/test-tmp/<unique>` (see `clipboard-privacy`).
- New copy: `Key::ClipDragged => "Ditarik", "Dragged"` (title of the notice after a thumbnail-only drop). Load `antislop:antislop` and `antislop:antislop-copywriting` before writing it.
- The history is not changed by a drag: no `bump`, no `set_raw`, no save.

## Review Focus

- A 4K screenshot (≈33 MB raw, or packed): the encode runs on the UI thread at drag start; it must finish, and a second drag of the same card within `KEEP` must reuse the file without encoding again. (Task 3 test: `touch` on a known id returns the existing path.)
- The temp folder cannot be written (no space, blocked): no drag starts, no panic, the press is dropped, and no `DragFiles` entry is left. (Task 2 test: `write_png` into a path under a file returns false.)
- The pressed card disappears or moves before the threshold (a new copy arrives, the list reorders): the press is keyed by card id, a missing id starts nothing. (Task 3 test on `drag_press_target`.)
- A 32 bpp DIB with all alpha bytes 0 (most screenshots) must come out opaque, not invisible; a DIB with real alpha keeps it. (Task 1 tests.)
- A removal that fails (a target still has the file open) is retried after `RETRY`, not forgotten. (Task 1 test on `expire` + re-`touch`.)

---

### Task 1: Pure drag module (name, pixels, bookkeeping)

**Files:**
- Create: `src/clipboard/drag.rs`
- Modify: `src/clipboard/mod.rs` (add `pub mod drag;`)

**Interfaces:**
- Produces:
  - `pub const KEEP: Duration` (600 s), `pub const RETRY: Duration` (60 s)
  - `pub fn file_name(what: &str, at: chrono::NaiveDateTime) -> String`
  - `pub struct PngPixels { pub w: u32, pub h: u32, pub bgra: bool, pub stride: u32, pub rows: Vec<u8> }` — top-down rows, no padding; `bgra` true = 4 bytes/pixel BGRA, false = 3 bytes/pixel BGR
  - `pub fn png_pixels(dib: &[u8]) -> Option<PngPixels>` (uses `image::dib_info`)
  - `pub struct DragFiles` with `pub fn touch(&mut self, id: u64, path: PathBuf, now: Instant) -> bool` (true when the id was already there: reuse its file), `pub fn path(&self, id: u64) -> Option<&Path>`, `pub fn expire(&mut self, now: Instant) -> Vec<(u64, PathBuf)>` (removes and returns entries whose deadline passed), `pub fn retain_ids(&mut self, live: &[u64]) -> Vec<(u64, PathBuf)>`, `pub fn take_all(&mut self) -> Vec<(u64, PathBuf)>`, `pub fn next_due(&self, now: Instant) -> Option<Duration>`, `pub fn retry(&mut self, id: u64, path: PathBuf, now: Instant)` (deadline `now + RETRY`), `pub fn is_empty(&self) -> bool`

- [ ] **Step 1: Write the failing tests** in `drag.rs`'s `#[cfg(test)] mod tests`:
  - `a_screenshot_name_reads_like_the_card`: `file_name("Screenshot", 2026-11-12 14:03:12)` == `"Screenshot 2026-11-12 14.03.12.png"`; `file_name("Gambar", ...)` starts with `"Gambar 2026-11-12"`; no `:` in either.
  - `a_bottom_up_32_bit_dib_comes_out_top_down`: 2×2 BI_RGB 32 bpp, positive height, distinct pixels → `rows` first 8 bytes are the DIB's *last* row; `bgra` true, `stride` 8.
  - `alpha_all_zero_comes_out_opaque`: every alpha byte 0 → every 4th byte of `rows` is 255.
  - `real_alpha_is_kept`: one pixel alpha 128, others 255 → 128 survives at its place.
  - `a_24_bit_dib_loses_its_row_padding`: 3×2 24 bpp (stride 12) → `bgra` false, `stride` 9, `rows.len()` 18.
  - `a_bitfields_dib_reads_past_its_masks`: 40-byte header + standard masks → same pixels as the BI_RGB case.
  - `garbage_gives_none`: `png_pixels(&[1, 2, 3])` is `None`.
  - `a_second_drag_reuses_the_file_and_restarts_the_clock`: `touch(7, p, t0)` false; `touch(7, p, t0 + 5 min)` true; `expire(t0 + 11 min)` empty; `expire(t0 + 16 min)` == `[(7, p)]`.
  - `next_due_is_the_nearest_deadline`: two ids touched at t0 and t0+2 min → `next_due(t0 + 1 min)` == 9 min; empty → `None`.
  - `a_deleted_card_gives_up_its_file`: ids 1, 2, 3 → `retain_ids(&[2])` returns 1 and 3, `path(2)` still `Some`.
  - `a_failed_removal_is_retried`: after `expire` returned `(5, p)`, `retry(5, p, t)` → `next_due(t)` == `RETRY`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --locked clipboard::drag`
Expected: FAIL (module does not exist yet).

- [ ] **Step 3: Implement** the items above. `png_pixels`: one pass over the rows of `dib_info` (flip when not `top_down`), copy `w * bpp/8` bytes per row; for 32 bpp, if no alpha byte is non-zero, set every alpha to 255. `DragFiles` is a `Vec<(u64, PathBuf, Instant)>` (a dozen entries at most).

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test --locked clipboard::drag`
Expected: PASS.

- [ ] **Step 5: Commit** `feat(clipboard): pure drag-out naming, pixels and temp-file bookkeeping`

### Task 2: Writing and removing the PNG (WIC)

**Files:**
- Modify: `src/clipboard/pack.rs` (generalise `encode`)
- Modify: `src/clipboard/drag.rs` (Win32 part)

**Interfaces:**
- Consumes: `PngPixels`, `png_pixels` (Task 1).
- Produces:
  - in `pack.rs`: `pub(super) fn encode_png(w: u32, h: u32, format: GUID, stride: u32, pixels: &[u8]) -> Option<Vec<u8>>`; `encode(layout, pixels)` becomes a call to it (behaviour unchanged, pack tests stay green).
  - in `drag.rs`: `pub fn root() -> PathBuf` (`temp_dir()\Vale\drag`), `pub fn card_path(root: &Path, id: u64, name: &str) -> PathBuf` (`root\<id>\name`), `pub fn write_png(dib: &[u8], path: &Path) -> bool` (creates the parent folder; `GUID_WICPixelFormat32bppBGRA` or `24bppBGR`; writes to `path`, removes a half-written file on failure), `pub fn remove(path: &Path) -> bool` (removes the file and its now-empty id folder; true when the file is gone afterwards, also when it never existed), `pub fn sweep(root: &Path)` (`remove_dir_all`, errors ignored).

- [ ] **Step 1: Write the failing tests** (COM initialised the way `pack.rs`'s tests do it; files under `target/test-tmp/clip-drag-<test>-<pid>`):
  - `a_written_png_decodes_to_the_same_size`: 3×2 32 bpp DIB → `write_png` true, file starts with the PNG signature `89 50 4E 47`, WIC decode gives 3×2.
  - `a_24_bit_dib_writes_too`: 3×2 24 bpp → true.
  - `a_bad_dib_writes_nothing`: `write_png(&[0; 8], p)` false and `p` does not exist.
  - `an_unwritable_place_writes_nothing`: parent path is an existing *file* → false, no panic.
  - `remove_takes_the_card_folder_with_it`: after `write_png` to `card_path(root, 9, "a.png")`, `remove` true and `root\9` is gone; `remove` of a missing path is true.
  - `sweep_clears_the_root`: two card folders → `sweep(root)` leaves no `root`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --locked clipboard::drag`
Expected: FAIL (functions missing).

- [ ] **Step 3: Implement** them; `write_png` = `png_pixels` → `pack::encode_png` → `std::fs::write`.

- [ ] **Step 4: Run** `cargo test --locked clipboard::` — Expected: PASS (pack tests included).

- [ ] **Step 5: Commit** `feat(clipboard): write a dragged image as a temp PNG`

### Task 3: Drag a card out of the Clipboard page

**Files:**
- Modify: `src/shelf_ole.rs` (`drag_out` signature)
- Modify: `src/app_clip.rs` (state, press, drag start, after drag, timer, lifetime hooks)
- Modify: `src/app_shelf.rs` (pass its effects; share the after-drag pointer check)
- Modify: `src/app.rs` (`on_press`, `on_release`, `on_mouse_leave`, `WM_MOUSEMOVE`, `WM_TIMER` dispatch, the timer-id uniqueness test)
- Modify: `src/i18n.rs` (`ClipDragged`)

**Interfaces:**
- Consumes: everything from Tasks 1–2.
- Produces:
  - `shelf_ole::drag_out(path: &Path, allowed: DROPEFFECT) -> Option<DROPEFFECT>` — `None` when the drag could not start, else the effect `DoDragDrop` reported. The shelf passes `COPY | LINK` and ignores the result.
  - `pub(super) const TIMER_CLIP_DRAG: usize = 55;` in `app_clip.rs`.
  - `ClipState` gains `press: Option<(u64, i32, i32)>` and `drag_files: DragFiles`.
  - `pub(super) struct ClipDragOut { pub path: PathBuf, pub small: bool }`
  - `fn drag_press_target(hit: Option<&Hit>, history: &History) -> Option<u64>` (pure: `Some(id)` only for `Hit::ClipCard(id)` whose entry is an image).
  - `fn past_threshold(from: (i32, i32), to: (i32, i32), min: f32) -> bool` (pure; move the shelf's inline maths here or into a shared helper and use it in both).
  - App methods: `clip_press(&mut self, hit: Option<&Hit>, lparam)`, `clip_release(&mut self)`, `clip_drag_start(&mut self, wparam, lparam) -> Option<ClipDragOut>`, `clip_after_drag_out(&mut self, small: bool, effect: Option<DROPEFFECT>)`, `clip_drag_expire(&mut self)` (the timer), `clip_drag_forget_missing(&mut self)`, `clip_drag_clear_all(&mut self)`.

Behaviour to wire:
- `on_press` calls `clip_press`; `on_release` and `on_mouse_leave` call `clip_release` (next to `shelf_release`).
- `clip_drag_start`: `None` and press cleared unless `primary_down(wparam)` and the view is `View::Panel(Page::Clipboard)`; `None` (press kept) under the threshold; past it: press cleared, `self.click.clear()`, entry looked up by id (gone → `None`); reuse `drag_files.path(id)` if the file still exists, else build the DIB as `win::formats_for` does (raw, unpack, or `dib_from_thumb`), `write_png` to `card_path(root(), id, file_name(..))` (failure → `None`); `touch`, arm the timer for `next_due`; `small = !img.has_original()`.
- `WM_MOUSEMOVE`: after the shelf's check, `if let Some(out) = with_app(|a| a.clip_drag_start(wparam, lparam)).flatten()` → `let effect = shelf_ole::drag_out(&out.path, DROPEFFECT_COPY);` → `with_app(|a| a.clip_after_drag_out(out.small, effect))`.
- `clip_after_drag_out`: the shelf's after-drag steps (cancel swipe, clear click, leave if the pointer ended off the island: share one helper with `shelf_after_drag_out`); when `small` and the effect is not `DROPEFFECT_NONE`, `apply_now(Event::Notice(..))` with key `"clip-dragged"`, source `ClipboardTitle`, title `ClipDragged`, body `i18n::clip_image_small`, kind `NoticeKind::Copied`.
- Timer `TIMER_CLIP_DRAG`: kill it, `expire(now)`, `remove` each; a failed one goes back with `retry`; re-arm for `next_due` if any.
- `clip_start`: `drag::sweep(&drag::root())` before anything else.
- `clip_shutdown`: `clip_drag_clear_all` (remove every file, kill the timer).
- `clip_on_lock`: when `clear_on_lock` is on, `clip_drag_clear_all` (even if no unpinned card was cleared).
- After a card delete and after "Hapus riwayat" (`clear_unpinned`): `clip_drag_forget_missing` (`retain_ids` with the live ids, remove what it returns).

- [ ] **Step 1: Write the failing tests** in `app_clip.rs`'s tests (and `shelf_ole.rs`):
  - `only_an_image_card_starts_a_drag`: history with a text (id 1) and an image (id 2) → `drag_press_target(Some(&Hit::ClipCard(2)), &h)` == `Some(2)`; id 1 → `None`; `Hit::ClipPin(2)` / `ClipDelete(2)` → `None`; an id not in the history → `None`; `None` hit → `None`.
  - `six_dip_starts_a_drag`: `past_threshold((0,0),(4,4),6.0)` false; `((0,0),(5,4),6.0)` true; `((10,10),(10,16),6.0)` true.
  - In `app.rs`'s timer-id test: add `clip::TIMER_CLIP_DRAG` to `ids` and `assert_eq!(clip::TIMER_CLIP_DRAG, 55);`.
  - `the_clipboard_drag_only_copies` (`shelf_ole.rs`): a `pub const CLIP_DRAG_EFFECTS: DROPEFFECT = DROPEFFECT_COPY` is used by the caller, and the test asserts it has neither the link nor the move bit.

- [ ] **Step 2: Run** `cargo test --locked drag` — Expected: FAIL (items missing).

- [ ] **Step 3: Implement** the interfaces and the wiring above. Use `CLIP_DRAG_EFFECTS` at the `WM_MOUSEMOVE` call site.

- [ ] **Step 4: Run** `cargo test --locked` — Expected: PASS, all tests (count grows by the new tests only).

- [ ] **Step 5: Build** `cargo build --release --locked` — Expected: builds; note `target/release/vale.exe` size for `exe-size`.

- [ ] **Step 6: Live check (agent, on the release build, owner's real data untouched):** stop the installed copy (`Stop-Process -Name vale, dynamic-island`), run `cargo run --release`, open the Clipboard page, drag an image card onto an Explorer folder: a `Screenshot ….png` file appears and opens; `%TEMP%\Vale\drag\<id>\` holds the temp file; quit Vale → the folder is gone. Record what could and could not be checked.

- [ ] **Step 7: Commit** `feat(clipboard): drag an image card out as a PNG file`
