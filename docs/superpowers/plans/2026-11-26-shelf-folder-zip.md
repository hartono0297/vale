# Shelf folder as zip on the phone page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A folder on the shelf shows on the phone/laptop link page as `<name>.zip` and downloads as a stored zip built while streaming.

**Architecture:** A new pure module `src/phone/zip_stream.rs` walks a folder into a `ZipPlan` (exact byte length known up front) and writes it to any `Write`. The share gate (`Handle::share`, `check_shared`) also accepts a plain folder. The server decides per request (like it does for files) whether a shared path is a file or a folder: a folder is listed with the plan's length and streamed through the plan. The page gets a folder icon and a "too many files" state.

**Tech Stack:** Rust 2024, std only (own CRC-32, `chrono` for DOS time), plain JS in `src/phone/page.html`.

**Spec:** `docs/superpowers/specs/2026-11-26-shelf-folder-zip-design.md`

## Global Constraints

- No new crate; `Cargo.toml` and `Cargo.lock` stay untouched (`cargo test --locked`).
- Never run `cargo fmt` on the repo; format new code by hand, match the file's style.
- Tests that spawn a process use `CREATE_NO_WINDOW` (`windows::Win32::System::Threading::CREATE_NO_WINDOW`), never show a window.
- Test temp folders go under `target/test-tmp/` like `files.rs` tests do.
- Max files in one folder zip: `MAX_FILES = 20_000` (more: listed as too big, never streamed).
- Known red tests on this PC, not caused by this work: `app::record::tests::a_folder_is_writable_only_when_a_file_can_be_made_in_it` (Z: drive); timing tests such as `recorder::tests::stop_pushes_the_rest_of_the_timeline` may fail under load and pass alone.
- Disk on D: is tight: run focused tests (`cargo test --locked phone::`), the full suite only once at the end; delete `target/test-tmp` after a full run.

## Review Focus

1. A file inside the folder is deleted, locked or grows while streaming: the stream must stop short (never send bytes that disagree with Content-Length or a wrong CRC silently). Task 1 test `a_file_that_changes_size_cuts_the_stream`.
2. A junction/symlink inside the folder (could loop or leak outside): skipped. Task 1 test `links_inside_are_skipped` (create a junction with `cmd /c mklink /J`, CREATE_NO_WINDOW; skip the assertion if mklink fails).
3. Non-ASCII names (`Laporan ü 日本.txt`): extract with the same name. Task 1 test `a_zip_extracts_to_the_same_tree`.
4. Large folders (> 4 GiB, > 65,535 entries) need ZIP64: Task 1 test with the threshold lowered.
5. A Range header on a folder download (a browser retrying): answered 200 full, not 206. Task 2 e2e assertion.

---

### Task 1: `zip_stream.rs`, folder to stored zip

**Files:**
- Create: `src/phone/zip_stream.rs` (code + `#[cfg(test)] mod tests`)
- Modify: `src/phone/mod.rs` (add `pub mod zip_stream;` to the module list, alphabetical)

**Interfaces:**
- Consumes: `super::files::open_shared(&Path) -> Result<Opened, OpenError>` (`Opened { file, len, mtime }`).
- Produces:
  - `pub const MAX_FILES: usize = 20_000;`
  - `pub enum Planned { Zip(ZipPlan), TooMany }`
  - `pub fn plan(root: &Path) -> Option<Planned>`: `None` when `root` is not a plain folder (missing, a file, a symlink/junction) or cannot be read.
  - `impl ZipPlan { pub fn len(&self) -> u64; pub fn newest(&self) -> Option<SystemTime>; pub fn write(&self, out: &mut dyn Write, keep_going: &mut dyn FnMut() -> bool) -> bool }`: `write` returns `true` only when every byte of `len()` was written; it calls `keep_going` before each chunk (64 KiB) and stops with `false` when it says no.
  - `pub fn crc32(seed: u32, bytes: &[u8]) -> u32` (running CRC-32/IEEE; `crc32(0, b"123456789") == 0xCBF4_3926`).

Format decisions (the implementer cannot infer these):
- Entries sorted by their relative path, `/` separators, folders as `name/` entries (size 0) so empty folders survive; no root prefix.
- Method 0 (stored). General purpose flags: bit 3 (data descriptor, CRC is known only after reading) | bit 11 (UTF-8). Local header carries CRC 0 and the planned sizes; the data descriptor (with signature `0x08074b50`) carries the CRC and sizes; the central directory carries the real CRC.
- ZIP64 per entry when its size or its local header offset is `>= zip64_at` (field on `ZipPlan`, default `0xFFFF_FFFF`, tests lower it): zip64 extra field (0x0001) in local and central headers, 8-byte sizes in its data descriptor; ZIP64 end record + locator when any entry needed it, the central directory offset/size is `>= zip64_at`, or the count is `>= 0xFFFF`.
- DOS time/date from each entry's mtime in local time (`chrono::Local`), clamped to 1980-01-01 for older times.
- While writing a file: open with `files::open_shared`; if it fails, or its opened `len` differs from the planned size, or reading hits EOF early, return `false`.
- Walk: `fs::symlink_metadata`; skip anything whose `file_type().is_symlink()` (covers junctions), skip unreadable subfolders, count files only toward `MAX_FILES`.

- [ ] **Step 1: Write the failing tests** in `zip_stream.rs`'s test module:
  - `crc32_matches_the_check_value`: `crc32(0, b"123456789") == 0xCBF4_3926`; `crc32(crc32(0, b"1234"), b"56789") == 0xCBF4_3926`.
  - `a_zip_extracts_to_the_same_tree`: folder with `a.txt` ("hello"), `sub/b.bin` (100 KiB of pattern bytes, > one chunk), `sub/empty/` (empty dir), `Laporan ü 日本.txt`; `plan` → `Zip`; write to a `Vec<u8>`; `write` returns true and `buf.len() as u64 == plan.len()`; save as `x.zip`, extract with `tar.exe -xf x.zip -C out` (CREATE_NO_WINDOW, status success); every file compares equal byte for byte and `out/sub/empty` is a dir; no `out/<root name>` prefix folder.
  - `an_empty_folder_is_a_valid_zip`: `plan` of an empty dir → `Zip`, written len == `len()`, `tar -tf` succeeds.
  - `zip64_records_extract_too`: same tree as above with `zip64_at` lowered to `1` (set the field in the test); written len == `len()`; tar extracts the same content.
  - `a_file_that_changes_size_cuts_the_stream`: plan a folder, then append to one file, `write` returns false.
  - `keep_going_false_stops_the_write`: `write` with a closure returning false → `false`, fewer bytes than `len()`.
  - `links_inside_are_skipped`: a junction `sub/loop` → the root (made with `cmd /c mklink /J`; if that fails, return early): listed entries contain no `loop`.
  - `too_many_files_is_reported`: a test-only lower cap is fine (e.g. a `plan_with_cap(root, cap)` private helper used by `plan`): 3 files with cap 2 → `TooMany`.
  - `a_file_or_missing_path_is_not_planned`: `plan(file) == None`, `plan(missing) == None`.

- [ ] **Step 2: Run** `cargo test --locked phone::zip_stream` → fails to compile / FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run** `cargo test --locked phone::zip_stream` → all PASS.
- [ ] **Step 5: Commit** `git add src/phone/zip_stream.rs src/phone/mod.rs` / `git commit -m "feat(phone): stored zip of a folder, streamed"`.

### Task 2: share and serve a folder

**Files:**
- Modify: `src/phone/files.rs` (add `pub fn plain_folder(path: &Path) -> bool`: `symlink_metadata` is a dir and not a symlink)
- Modify: `src/phone/mod.rs` (`Handle::share` doc + gate)
- Modify: `src/phone/server.rs` (`check_shared`, `kind_word`, `file_list`, `download`)
- Modify: `src/phone/share.rs` (doc comments that say "regular file" only, if they become wrong)
- Test: `src/phone/files.rs` tests, `src/phone/e2e.rs`

**Interfaces:**
- Consumes: Task 1's `zip_stream::{plan, Planned, ZipPlan}`.
- Produces: `/api/files` entries for a folder: `{"id", "name": "<folder>.zip", "size": plan.len(), "v": etag(len, newest) without quotes, "kind": "folder", "view": false, "saved", "busy"}`; a folder over the cap: same with `"size": 0, "big": true`. `GET/HEAD /api/file/<id>` of a folder: 200, `Content-Type: application/zip`, `Content-Length: plan.len()`, `Content-Disposition` attachment with `<folder>.zip`, `Accept-Ranges: none`, `Cache-Control: no-store`, no ETag; Range/If-Range ignored. A too-big folder: `Response::error(500, "too many files")`. `/api/view/<id>` and `/api/thumb/<id>` of a folder: 404.

Rules:
- `Handle::share`: local path shared when `files::regular_size(path).is_some() || files::plain_folder(path)`; network paths unchanged (`share_unseen`). `check_shared` revokes only when neither holds.
- `file_list` and `download` decide per request: regular file → today's path untouched; else plain folder → zip; else gone (revoke, 410) as today.
- Folder streaming: same `Busy` guard, `keep_going` closure = not stopped, id still shared (`contains`), `touch`; after `write` returns true on a GET, `mark_saved(id, device)`.
- `kind_word(ShelfKind::Folder)` → `"folder"` (`Document` stays `"doc"`).

- [ ] **Step 1: Write the failing tests**
  - `files.rs`: `plain_folder_is_true_only_for_a_real_folder` (dir true; file, missing, junction (mklink /J, skip if it fails) false).
  - `e2e.rs` line ~580: change `assert!(r.handle.share(&share).is_none(), "a folder")` to expect `Some` where the test needs it, or move that check into the new test and keep the old test's flow intact (its file counts must not change — share a sibling folder instead).
  - `e2e.rs` new `a_shelf_folder_downloads_as_a_zip`: rig, folder `DA` with `a.txt` and `sub/b.txt`; `share` → `Some(id)`; `files_of` has an entry with name `DA.zip`, kind `folder`, view false, size > 0; `HEAD /api/file/<id>` → 200, `content-length` == listed size, `accept-ranges: none`, content-disposition contains `DA.zip`; `GET` with `Range: bytes=10-` → 200 and body length == listed size; body saved and `tar -tf` lists `a.txt` and `sub/b.txt`; `saved(r, vs, id)` true afterwards; `GET /api/view/<id>` → 404; delete the folder → next `files_of` has no entry and a `SharedGone` event arrives.
- [ ] **Step 2: Run** `cargo test --locked phone::` → new tests FAIL.
- [ ] **Step 3: Implement** the rules above.
- [ ] **Step 4: Run** `cargo test --locked phone::` → all PASS.
- [ ] **Step 5: Commit** `git commit -m "fix(phone): share shelf folders as a zip"` (the files above only).

### Task 3: page shows folders

**Files:**
- Modify: `src/phone/page.html` only

**Interfaces:**
- Consumes: `/api/files` entries with `kind: "folder"` and optional `big: true` (Task 2).

- [ ] **Step 1:** Add `folder:"M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"` to `KIND`.
- [ ] **Step 2:** Add string `many` (the key `big` is taken by uploads) to both dictionaries: Indonesian `"Isinya terlalu banyak"`, English `"Too many files"`.
- [ ] **Step 3:** `fileRow`: when `f.big` (and not gone/err), the action is `h("span",{class:"gn"},t("many"))` instead of a Save button; `unsaved(f)` also returns false for `f.big` so Save all skips it. Keep the minified style of the file.
- [ ] **Step 4: Run** `cargo test --locked phone::` (the page is embedded; its tests must stay green).
- [ ] **Step 5:** Do not commit (the controller commits it).
