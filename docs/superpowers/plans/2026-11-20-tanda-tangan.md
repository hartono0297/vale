# Tanda tangan Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sign and lightly edit PDFs, images and Office files from the file shelf, in a window of its own, fully offline.

**Architecture:** A new `src/sign/` module. Pure parts (geometry, session model, naming, PDF writer, signature store) are unit-tested without windows. Two workers do the Windows-only I/O: a renderer (WinRT `Windows.Data.Pdf` to PNG to WIC pixels) and an Office converter (COM / headless LibreOffice). A Win32 + Direct2D window (`src/sign/window/`) ties them together; `src/app_sign.rs` wires it to the shelf menu, the notch notice and the shelf.

**Tech Stack:** Rust 2024, `windows` 0.62 (Direct2D, DirectWrite, WIC, COM, WinRT `Data_Pdf`), `lopdf` 0.47 with `default-features = false`, existing `src/dpapi.rs`.

**Spec:** `docs/superpowers/specs/2026-11-20-tanda-tangan-design.md` (mockup `docs/design/tanda-tangan.html`). Executors read both.

## Global Constraints

- Windows 11 only; Rust stable 1.99 (lopdf 0.47 needs ≥ 1.88).
- `lopdf = { version = "0.47", default-features = false }`. Never enable `embed_image`, `image`, `font_embedding`, `rayon`, `chrono-clock`.
- New `windows` features: `Data_Pdf` (pulls `Data`). `Storage_Streams`, `Foundation`, `Win32_System_WinRT`, `Win32_System_Com`, `Win32_System_Variant`, `Win32_System_Ole` already exist.
- Size gate: the whole module may add at most **1,363,148 bytes** (1.3 MiB, owner's cap) to `vale.exe` over the baseline measured in Task 1. Over that: stop and report to the owner, do not trim features on your own.
- Never touch `src/shot_edit/` (another session edits it). Copy code from it; calling its `pub` helpers (`shot_edit::gfx::bitmap`, etc.) is allowed.
- Nothing goes over the network. No registry writes.
- Idle cost zero: no timer on the main window, no thread alive while no signing window is open.
- Tests never show windows: `const ON_SCREEN: bool = !cfg!(test);` guards `ShowWindow`, `SetForegroundWindow`, `SetFocus`. Test temp files go under `target/` via `crate::phone::files::tests::temp_dir(name)`, never `%APPDATA%`.
- Never run `cargo fmt` on the repo; format new code by hand. Run `cargo test --locked` (whole suite) before each commit.
- No em dash in any UI string. Indonesian is the source copy (mockup wording); every `Key` has `id` and `en`.
- `panic = "abort"`: no `unwrap`/`expect`/indexing that can panic on file content. Every lopdf / WinRT / COM error becomes an `Err` the UI shows.

## Review Focus

1. **Real-world PDFs from other tools** (bank statements, Acrobat-encrypted AES-256, xref streams, broken xref): open, render, save, reopen in Windows' renderer. Expected: either works or a clear "Dokumen ini tidak bisa dibuka" message, never a crash. Test owner: Task 3 (`writes_into_xref_stream_source`, `garbage_and_truncated_input_are_errors`).
2. **Deleted page really gone**, including on encrypted PDFs (full save with re-encryption). Test owner: Task 3 (`deleted_page_content_is_absent_after_save`, `deleted_page_on_encrypted_pdf_keeps_password`).
3. **Rotated source pages** (`/Rotate 90` already in the file, or inherited from `/Pages`): a signature placed upright on screen is upright in the output. Test owner: Task 2 (`placing_on_a_page_shown_rotated_is_upright`) and Task 3 (`item_on_rotated_source_page_gets_matching_matrix`).
4. **Office already open by the user** (Word running with their own document): conversion must not close it or their document. Test owner: Task 6 (`quit_only_when_we_started_the_process`).
5. **Output folder not writable or file name taken** (Downloads synced, read-only USB, `X (ditandatangani).pdf` exists): never overwrites, falls back to `Documents\Vale\Ditandatangani`. Test owner: Task 1 (`numbers_when_taken`, `falls_back_when_folder_is_read_only`).

---

## File structure

| File | Responsibility | Task |
|---|---|---|
| `Cargo.toml`, `Cargo.lock` | lopdf, `Data_Pdf` | 1 |
| `src/i18n.rs` | every new `Key` | 1 |
| `src/sign/mod.rs` | shared types (`Source`, `Kind`, `Item`, `Mark`, `Quarter`), module list, `is_signable` | 1 |
| `src/sign/names.rs` | output path: `(ditandatangani)` naming, numbering, fallback folder | 1 |
| `src/sign/window/mod.rs` | stub `open`/`release`/`restyle_open`/`run_demo` (real in Task 8) | 1 |
| `src/sign/geom.rs` | page space ↔ view space, item matrices | 2 |
| `src/sign/session.rs` | editing model: pages, items, undo/redo, rotate/delete | 2 |
| `src/sign/pdf_write.rs` | lopdf: inspect (encryption, permissions, signatures) and save | 3 |
| `src/sign/winansi.rs` | UTF-8 → WinAnsi (cp1252) bytes or `None` | 3 |
| `src/sign/image_out.rs` | image input → PNG/JPG with items composited, or → PDF | 4 |
| `src/sign/render.rs` | render worker (Windows.Data.Pdf, WIC images) | 5 |
| `src/sign/office.rs` | Office / WPS / LibreOffice → PDF | 6 |
| `src/sign/ink.rs` | strokes / typed name / photo → `Mark` (RGBA, cropped) | 7 |
| `src/sign/store.rs` | saved signatures, DPAPI-sealed | 7 |
| `src/sign/window/*.rs` | the signing window and the signature maker | 8 |
| `src/app_sign.rs`, `src/app.rs`, `src/ui/tray.rs`, `src/main.rs` | shelf menu, open, notice, add to shelf, sweep, demo flag, restyle/release hooks | 9 |
| `THIRD-PARTY-NOTICES.md` | lopdf and its crates | 10 |

**Order and parallelism:** Task 1 first. Then Tasks 2, 5, 6 and 7 in parallel. Tasks 3 and 4 in parallel once Task 2 is merged (they use `geom`). Tasks 8 and 9 in parallel once 2-7 are merged (Task 9 codes against the Task 1 stub, so it does not wait for 8). Task 10 last. Each parallel task in its own worktree under `.claude/worktrees/`, touching only its own files. Merge into `claude/tanda-tangan` one at a time: merge the latest branch tip in first, run `cargo test --locked`.

---

### Task 1: Foundation (deps, shared types, strings, naming, size baseline)

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `src/i18n.rs`, `src/main.rs` (only `mod sign;`)
- Create: `src/sign/mod.rs`, `src/sign/names.rs`, `src/sign/window/mod.rs`
- Test: in-file `#[cfg(test)]` in `src/sign/mod.rs`, `src/sign/names.rs`

**Interfaces:**
- Produces (in `src/sign/mod.rs`):
  - `pub enum Kind { Pdf, Image, Office }`
  - `pub struct Source { pub path: PathBuf, pub kind: Kind }`
  - `pub fn kind_of(path: &Path) -> Option<Kind>`: lowercased extension; `pdf` → Pdf; `png jpg jpeg` → Image; `doc docx xls xlsx ppt pptx` → Office; else `None`.
  - `pub fn is_signable(path: &Path) -> bool` = `kind_of(path).is_some()`.
  - `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Quarter { R0, R90, R180, R270 }` with `fn from_degrees(i64) -> Quarter` (normalises any multiple of 90, others → R0), `fn degrees(self) -> i64`, `fn add(self, other) -> Quarter`, `fn turn_cw(self) -> Quarter`.
  - `pub struct Mark { pub w: u32, pub h: u32, pub rgba: Vec<u8> }`: straight (not premultiplied) RGBA, top-down, cropped to ink.
  - `pub enum ItemKind { Signature(Arc<Mark>), Initials(Arc<Mark>), Date(String), Text(String), Check }`
  - `pub struct Item { pub kind: ItemKind, pub page: usize, pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub turn: Quarter }`: `x, y` = lower-left corner in **unrotated page space** (PDF points, origin at the page box's lower-left), `w, h` = size in the item's own upright frame, `turn` = rotation of the item relative to page space (counter-clockwise quarters, as PDF `cm` applies).
- Produces (in `src/sign/names.rs`):
  - `pub fn output_path(src: &Path, ext: &str, lang: Lang, exists: impl Fn(&Path) -> bool, writable: impl Fn(&Path) -> bool, fallback: &Path) -> PathBuf`
  - `pub fn fallback_dir() -> PathBuf`: `Documents\Vale\Ditandatangani` via `SHGetKnownFolderPath(FOLDERID_Documents)`.
- Produces (stub, `src/sign/window/mod.rs`): `pub fn open(owner: HWND, src: Source, demo: bool) -> bool { false }`, `pub fn release() {}`, `pub fn restyle_open() {}`, `#[cfg(debug_assertions)] pub fn run_demo() {}`, `pub const WM_APP_SIGN: u32 = WM_APP + 96;`, `pub enum Report { Saved(PathBuf) }`, `pub fn take_reports() -> Vec<Report> { Vec::new() }`.

- [ ] **Step 1: Measure the baseline.** On the unchanged branch: `cargo build --release --locked`, then `(Get-Item target/release/vale.exe).Length`. Write the number into the task's commit message and into `docs/superpowers/plans/2026-11-20-tanda-tangan.md` under "Size log" (append at the end). Expected: a number near 5,0 MB.
- [ ] **Step 2: Add deps.** `lopdf = { version = "0.47", default-features = false }` in `[dependencies]`; `"Data_Pdf"` in the `windows` features list (after `"Foundation_Collections"`). Try `miniz_oxide = "0.9"` (lopdf's flate2 uses 0.9; one copy instead of two): if `cargo test --locked` (after `cargo update -p miniz_oxide --offline` or a normal lockfile update) compiles and passes, keep it; otherwise revert to `"0.8"` and note it in the Size log. Run `cargo build --release` once so `Cargo.lock` updates, then use `--locked` from here on.
- [ ] **Step 3: Write failing tests** in `src/sign/mod.rs`:
  - `kind_of_by_extension`: `kind_of("a.PDF") == Some(Pdf)`, `"b.jpeg"` → Image, `"c.Docx"` → Office, `"d.pptx"` → Office, `"e.heic"` → None, `"f"` → None, `"g.pdf.txt"` → None.
  - `quarter_normalises`: `from_degrees(-90) == R270`, `from_degrees(450) == R90`, `from_degrees(45) == R0`, `R270.turn_cw() == R0`, `R90.add(R270) == R0`.
- [ ] **Step 4: Write failing tests** in `src/sign/names.rs`:
  - `id_name`: `output_path("C:/d/Kontrak sewa.pdf", "pdf", Id, |_| false, |_| true, fb)` == `C:/d/Kontrak sewa (ditandatangani).pdf`.
  - `en_name`: same with `En` → `Kontrak sewa (signed).pdf`.
  - `numbers_when_taken`: exists returns true for the plain name and ` (2)` → `Kontrak sewa (ditandatangani) (3).pdf`.
  - `image_keeps_or_changes_ext`: `"Surat.JPG"`, ext `"jpg"` → `Surat (ditandatangani).jpg`; ext `"pdf"` → `.pdf`.
  - `falls_back_when_folder_is_read_only`: `writable` false → path under `fb` with the same file name rule.
  - `office_source_names_from_original`: `"Penawaran.docx"`, ext `"pdf"` → `Penawaran (ditandatangani).pdf` next to the `.docx`.
- [ ] **Step 5: Run** `cargo test --locked sign::` → FAIL (missing items).
- [ ] **Step 6: Implement** the types and `names.rs`. Numbering ` (2)`..` (999)`, then ` (1000)` like `screenshot::save_name`. `writable(dir)` in production = try `OpenOptions::create_new` of a `.vale-probe-<pid>` file and delete it.
- [ ] **Step 7: Add i18n keys** in the `keys!` block (`src/i18n.rs:110`), each `id, en`. Add every one to the "differs between languages" test list (around `src/i18n.rs:5540`) unless both strings are identical by design.

  | Key | id | en |
  |---|---|---|
  | `SignMenu` | Tanda tangani | Sign |
  | `SignPageOf` | Halaman {n} dari {total} | Page {n} of {total} |
  | `SignToolSignature` | Tanda tangan | Signature |
  | `SignToolInitials` | Paraf | Initials |
  | `SignToolDate` | Tanggal | Date |
  | `SignToolText` | Teks | Text |
  | `SignToolCheck` | Centang | Check |
  | `SignRotate` | Putar | Rotate |
  | `SignDeletePage` | Hapus halaman | Delete page |
  | `SignSmaller` | Lebih kecil | Smaller |
  | `SignBigger` | Lebih besar | Bigger |
  | `SignAddDate` | + Tanggal | + Date |
  | `SignRemove` | Hapus | Remove |
  | `SignAllPages` | Pasang di semua halaman | Put on every page |
  | `SignSaveAs` | Disimpan sebagai {name}, di folder yang sama | Saved as {name}, in the same folder |
  | `SignSaveAsFallback` | Disimpan sebagai {name}, di Dokumen\Vale\Ditandatangani | Saved as {name}, in Documents\Vale\Ditandatangani |
  | `SignSave` | Simpan | Save |
  | `SignCancel` | Batal | Cancel |
  | `SignFormatImage` | Gambar | Image |
  | `SignFormatPdf` | PDF | PDF |
  | `SignMakerDraw` | Gambar | Draw |
  | `SignMakerType` | Ketik | Type |
  | `SignMakerPhoto` | Dari foto | From photo |
  | `SignMakerRedo` | Ulangi | Start over |
  | `SignMakerInk` | Tinta | Ink |
  | `SignMakerKeep` | Simpan di PC ini | Keep on this PC |
  | `SignMakerKeepHint` | Terkunci ke akun Windows ini. Orang lain di PC yang sama tidak bisa membukanya. | Locked to this Windows account. Other people on the same PC cannot open it. |
  | `SignMakerNew` | + Baru | + New |
  | `SignMakerUse` | Pakai | Use |
  | `SignMakerTypeHint` | Ketik nama | Type your name |
  | `SignMakerPhotoPick` | Pilih foto tanda tangan di kertas | Pick a photo of a signature on paper |
  | `SignPassword` | Dokumen ini dikunci. Masukkan kata sandinya. | This document is locked. Enter its password. |
  | `SignPasswordWrong` | Kata sandinya salah. | Wrong password. |
  | `SignOpen` | Buka | Open |
  | `SignNoChanges` | Pembuat dokumen ini tidak mengizinkan perubahan. | The author of this document does not allow changes. |
  | `SignBreaksSignature` | Tanda tangan digital yang sudah ada di dokumen ini jadi tidak berlaku. | The digital signature already in this document will no longer be valid. |
  | `SignSaveAnyway` | Simpan tetap | Save anyway |
  | `SignCannotKeepLock` | Kunci dokumen ini tidak bisa dipertahankan, jadi tidak disimpan. | This document's lock cannot be kept, so it was not saved. |
  | `SignCannotOpen` | Dokumen ini tidak bisa dibuka. | This document cannot be opened. |
  | `SignConverting` | Mengubah ke PDF… | Converting to PDF… |
  | `SignNoOffice` | Tidak ada Office di PC ini. Simpan dulu sebagai PDF dari aplikasinya, lalu coba lagi. | No office app on this PC. Save it as PDF from its app first, then try again. |
  | `SignOfficeFailed` | Gagal mengubah ke PDF. Simpan dulu sebagai PDF dari aplikasinya. | Could not convert to PDF. Save it as PDF from its app first. |
  | `SignOfficeLocked` | Dokumen Office ini dikunci kata sandi. Simpan dulu sebagai PDF dari aplikasinya. | This Office document is password-locked. Save it as PDF from its app first. |
  | `SignSaving` | Menyimpan… | Saving… |
  | `SignSaveFailed` | Gagal menyimpan. File aslinya tidak berubah. | Could not save. The original file is unchanged. |
  | `SignLastPage` | Halaman terakhir tidak bisa dihapus. | The last page cannot be deleted. |
  | `SignDone` | Ditandatangani | Signed |
  | `SignNoticeBody` | {name} · klik untuk buka | {name} · click to open |
  | `SignTooBig` | File ini terlalu besar untuk dibuka (lebih dari 200 MB). | This file is too big to open (over 200 MB). |
  | `SignDiscard` / `SignKeep` | reuse `Key::EditDiscard` / `Key::EditKeep`, no new key | |
  | `Month1`..`Month12` | only if no month-name keys exist yet (grep `Januari` in `src/i18n.rs` first and reuse) | |

- [ ] **Step 8: Create the stub** `src/sign/window/mod.rs` with the signatures above.
- [ ] **Step 9: Run** `cargo test --locked` → PASS (whole suite).
- [ ] **Step 10: Commit** `feat(sign): foundation types, naming, strings, lopdf dependency` (message includes the baseline size).

---

### Task 2: Geometry and the editing session

**Files:**
- Create: `src/sign/geom.rs`, `src/sign/session.rs`
- Test: in-file

**Interfaces:**
- Consumes: Task 1 types.
- Produces (`geom.rs`):
  - `#[derive(Clone, Copy)] pub struct PageGeom { pub w: f32, pub h: f32, pub base: Quarter }`: `w, h` = unrotated page box size in points; `base` = the source's own `/Rotate`.
  - `pub fn shown_size(g: PageGeom, extra: Quarter) -> (f32, f32)`: displayed width/height after `base + extra`.
  - `pub fn page_to_view(g: PageGeom, extra: Quarter, scale: f32, p: (f32, f32)) -> (f32, f32)`: page space (y up) → view pixels (y down, origin top-left of the shown page).
  - `pub fn view_to_page(g, extra, scale, v) -> (f32, f32)`: exact inverse.
  - `pub fn upright_turn(g: PageGeom, extra: Quarter) -> Quarter`: the `turn` a new item needs to look upright in the current view (= inverse of `base + extra`).
  - `pub fn item_matrix(item: &Item) -> [f32; 6]`: the PDF `cm` matrix mapping the unit square to the item in page space (translate to `x, y`, rotate by `turn`, scale `w, h`; for R90/R270 the occupied box is `h × w`).
  - `pub fn item_view_rect(item: &Item, g, extra, scale) -> (f32, f32, f32, f32)`: axis-aligned view rect for hit-testing and handles.
- Produces (`session.rs`):
  - `pub struct Page { pub geom: PageGeom, pub extra: Quarter, pub deleted: bool }`
  - `pub struct Session { pub pages: Vec<Page>, pub items: Vec<Item>, ... }` with `new(geoms: Vec<PageGeom>) -> Session`, `add(&mut self, item: Item) -> usize`, `move_item(i, dx, dy)` (page space), `resize_item(i, w, h)` (keeps aspect for marks, min 8 pt, max the page's shorter side), `remove_item(i)`, `rotate_page(p)` (extra.turn_cw), `delete_page(p) -> Result<(), LastPage>` (refuses the last live page; removes that page's items), `initials_on_all(i)` (copies an Initials item to every other live page at the same relative spot, skipping pages where it would fall outside the shown box), `undo() -> bool`, `redo() -> bool`, `dirty() -> bool`, `live_pages() -> Vec<usize>`, `has_deleted() -> bool`, `has_rotated() -> bool`. Undo stores whole snapshots of `(pages, items)` like the screenshot editor (`shot_edit/mod.rs:462`).

- [ ] **Step 1: Write failing tests** (`geom.rs`):
  - `roundtrip_all_quarters`: for `base` and `extra` in all four quarters, `view_to_page(page_to_view(p)) ≈ p` (ε 0.01) on a 612×792 page, scale 1.5.
  - `shown_size_swaps_on_odd_quarters`: `shown_size({612,792,R90}, R0) == (792, 612)`; `({612,792,R90}, R90) == (612, 792)`.
  - `placing_on_a_page_shown_rotated_is_upright`: page `{612,792,R90}`, `upright_turn == R270`; an item with that turn, `item_view_rect` has width ≈ `w*scale`, height ≈ `h*scale`.
  - `item_matrix_r0`: item at (100, 200), 150×50, R0 → `[150, 0, 0, 50, 100, 200]`.
  - `item_matrix_r90`: same item, R90 → `[0, 150, -50, 0, 100+50, 200]` (unit square's x axis goes up, occupied box 50 wide, 150 tall, lower-left stays at (100, 200)).
- [ ] **Step 2: Write failing tests** (`session.rs`):
  - `rotate_turns_items_with_content`: add an item, `rotate_page(0)`; the item's `x, y, turn` are unchanged and `pages[0].extra == R90`.
  - `delete_refuses_last_page`: 1 page → `Err(LastPage)`.
  - `delete_removes_items_of_that_page`.
  - `undo_redo_restores_snapshot`: add, move, undo → original position; redo → moved; a new edit clears redo.
  - `initials_on_all_copies_to_live_pages`: 3 pages of 612×792, page 2 deleted, initials on page 1 at (500, 40) → one copy, on page 3, same `x, y`.
  - `initials_on_all_skips_page_where_it_falls_outside`: page 3 is 300×300 → no copy on page 3.
  - `resize_keeps_aspect_and_limits`.
- [ ] **Step 3: Run** `cargo test --locked sign::` → FAIL; **implement**; run → PASS; whole suite PASS.
- [ ] **Step 4: Commit** `feat(sign): page geometry and editing session`.

---

### Task 3: PDF inspect and save (lopdf)

**Files:**
- Create: `src/sign/pdf_write.rs`, `src/sign/winansi.rs`
- Test: in-file; fixtures are generated in the tests with lopdf (no binary fixtures).

**Interfaces:**
- Consumes: Task 1 types, Task 2 `geom::item_matrix`, `session::Page`.
- Produces:
  - `pub struct Opened { pub doc: lopdf::Document, pub bytes: Vec<u8>, pub geoms: Vec<PageGeom>, pub encrypted: bool, pub no_changes: bool, pub signed: bool }`
  - `pub enum OpenError { NeedsPassword, WrongPassword, Broken, TooBig }`
  - `pub fn open(bytes: Vec<u8>, password: Option<&str>) -> Result<Opened, OpenError>`: rejects > 200 MiB (`TooBig`); `load_mem_with_password` when a password is given, else `load_mem` and `is_encrypted()` with no state → `NeedsPassword` (try the empty user password first: `load_mem_with_password(bytes, "")`); geometry from `/CropBox` else `/MediaBox` (inherited through `/Parent`), `/Rotate` inherited; `no_changes` = `/P` lacks the MODIFY bit (bit 4) or ANNOTATE (bit 6) per `encryption_state.permissions()`; `signed` = `/AcroForm /SigFlags` has bit 1, or any object dict with `/FT /Sig` and a `/V`.
  - `pub enum SaveError { CannotKeepLock, Write(String) }`
  - `pub fn save(o: &Opened, pages: &[Page], items: &[Item]) -> Result<Vec<u8>, SaveError>`:
    - No deleted page → incremental: `IncrementalDocument::create_from(o.bytes.clone(), o.doc.clone())`; for each touched page `opt_clone_object_to_new_document(page_id)`, set `/Rotate` = `base + extra` degrees when `extra != R0`, append a content stream (wrap existing `/Contents` into an array by hand: `[old..., new]`), add XObjects / Helvetica to the page's resources (clone the resources dict into the new revision if it is a reference). The encryption of the source carries over (verified in the spike for RC4-40/128, AES-128, AES-256).
    - Any deleted page → full rewrite: `let state = doc.encryption_state.clone()`; apply edits on a clone; `delete_pages`; `prune_objects()`; if `state` is `Some`: `doc.encryption_state = None; doc.encrypt(&state)` and on any error `Err(CannotKeepLock)`; `save_to`.
    - Content stream per page: `q` + per item `q <item_matrix> cm` + draw + `Q`, closing `Q`. Signature/Initials: Image XObject `/DeviceRGB` 8 bpc Flate (compress with `miniz_oxide::deflate::compress_to_vec_zlib(…, 6)`) + `/SMask` DeviceGray from alpha; resource names `/ValeS0`, `/ValeS1`, … (never clash: skip names already in the page's `/XObject`). Date / Text: `BT /ValeF 1 Tf … Tj ET` with Helvetica `WinAnsiEncoding`, font size = item `h * 0.8`, if `winansi::encode` returns `None` the caller (window) has already turned the text into a Mark, so the writer only gets encodable text. Check: a stroked path, line width 0.12 of the unit box, colour `0.12 0.23 0.54 RG` (ink blue `#1F3A8A`).
    - Before returning, reload the output with `load_mem_with_password` (same password) to prove it parses; failure → `Err(Write)`.
  - `winansi.rs`: `pub fn encode(s: &str) -> Option<Vec<u8>>` (cp1252, including `€ ‚ ƒ „ … † ‡ ˆ ‰ Š ‹ Œ Ž ‘ ’ “ ” • – — ˜ ™ š › œ ž Ÿ` in 0x80-0x9F), `pub fn pdf_string(bytes: &[u8]) -> Vec<u8>` (escape `\ ( )`).

- [ ] **Step 1: Write failing tests:**
  - `opens_plain_pdf_geometry`: a generated 2-page PDF (612×792, second page `/Rotate 90`) → `geoms == [{612,792,R0},{612,792,R90}]`, `encrypted == false`.
  - `inherited_rotate_and_cropbox`: `/Rotate 270` on `/Pages`, `/CropBox [10 10 500 700]` on the page → `{490, 690, R270}`.
  - `adds_signature_incrementally_keeps_prefix`: save with one Signature item; output starts with the original bytes; reload, page 1 has an XObject whose name starts with `ValeS` and a `/SMask`.
  - `item_on_rotated_source_page_gets_matching_matrix`: on the `/Rotate 90` page, the new content stream contains the `cm` numbers from `geom::item_matrix`.
  - `text_item_uses_helvetica_winansi`: Date item `"20 November 2026"` → resources hold a Type1 Helvetica with `WinAnsiEncoding` and the content contains `(20 November 2026) Tj`.
  - `rotate_sets_page_rotate`: `extra = R90` on a `/Rotate 90` page → reloaded `/Rotate 180`.
  - `deleted_page_content_is_absent_after_save`: page 2's content contains the marker `(VALE-PAGE-TWO) Tj`; delete page 2; output bytes do not contain `VALE-PAGE-TWO`, page count 1.
  - `encrypted_incremental_keeps_password`: for RC4-128 and AES-256 generated with `Document::encrypt`: open with "rahasia", add an item, save; `load_mem` without password stays encrypted / unreadable, `load_mem_with_password("rahasia")` shows the new XObject.
  - `deleted_page_on_encrypted_pdf_keeps_password`: AES-256, delete a page → output needs "rahasia", page count drops, marker absent after decrypt.
  - `wrong_password_is_reported`, `needs_password_without_one`.
  - `permission_flags_set_no_changes`: encrypted with permissions lacking MODIFIABLE → `no_changes == true`; with all → false.
  - `detects_existing_signature`: AcroForm `/SigFlags 3` + a `/FT /Sig` field with `/V` → `signed == true`.
  - `writes_into_xref_stream_source`: a source saved with lopdf's modern (xref stream + object streams) writer → incremental save reloads and shows the item.
  - `garbage_and_truncated_input_are_errors`: `b"hello"` → `Err(Broken)`; a valid PDF cut 40 bytes short → `Ok` or `Err(Broken)`, never a panic (run in the test, the process would abort on panic).
  - `winansi_encodes_indonesian_and_rejects_cjk`: `"Rp 1.500 – Ibu Siti"` → `Some`, `"李"` → `None`, `"€"` → `Some(vec![0x80])`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS, whole suite PASS.
- [ ] **Step 5: Commit** `feat(sign): open and save PDFs with lopdf, keep encryption`.

---

### Task 4: Image input output

**Files:**
- Create: `src/sign/image_out.rs`
- Test: in-file

**Interfaces:**
- Consumes: Task 1 types, Task 2 `geom`, Task 7 `raster` (offscreen D2D on WIC), `crate::shelf_thumb::decode`-style WIC decode (copy the needed part; full size, cap 250 M pixels), `crate::screenshot::win::encode_png`, JPEG encode copied from `src/phone/thumb.rs`.
- Produces:
  - `pub fn image_geom(w_px: u32, h_px: u32) -> PageGeom`: points = px × 0.75, base R0.
  - `pub enum ImageFormat { Png, Jpg, Pdf }`
  - `pub fn compose(src_bgra: &[u8], w: u32, h: u32, page: &Page, items: &[Item], marks_for_text: &dyn Fn(&str, f32) -> Mark) -> (u32, u32, Vec<u8>)`: draws items onto a copy of the pixels with `raster::offscreen` (draw the source as a bitmap first), then applies `page.extra` by rotating pixels. Text/date drawn via DirectWrite "Arial" at `h × 0.8` pt × (1/0.75) px, ink blue; check as a path.
  - `pub fn new_pdf_from_image(img: &Mark, w_pt: f32, h_pt: f32) -> Vec<u8>`: one-page lopdf PDF with the image full-page (Flate RGB, no SMask needed for opaque input).
  - `pub fn save_image(composed: (u32, u32, Vec<u8>), fmt: ImageFormat) -> Result<Vec<u8>, String>`: PNG via `encode_png` (keeps opaque), JPG quality 0.92, PDF via `new_pdf_from_image` with the page in points.
- [ ] **Step 1: Failing tests** (COM initialised in the test; no window):
  - `geom_is_three_quarters_of_pixels`: 1600×1200 → `{1200, 900, R0}`.
  - `compose_puts_signature_pixels_at_item`: 400×300 white image, a 40×20 pt fully-opaque blue Mark at page (30, 30): the composed pixel at the item's centre (view coords via `geom`) is blue (B > 150, R < 80); a corner far away stays white.
  - `rotation_rotates_output`: `extra = R90` on 400×300 → output 300×400.
  - `pdf_from_image_has_one_page_of_matching_size`: reload with lopdf; MediaBox `[0 0 300 225]` for a 400×300 image.
  - `jpg_and_png_headers`: PNG starts with `\x89PNG`, JPG with `FF D8`.
- [ ] **Step 2-4:** Run FAIL, implement, run PASS + whole suite.
- [ ] **Step 5: Commit** `feat(sign): image input and output`.

---

### Task 5: Render worker

**Files:**
- Create: `src/sign/render.rs`
- Test: in-file (uses a lopdf-generated PDF; runs headless)

**Interfaces:**
- Produces:
  - `pub struct Pixels { pub w: u32, pub h: u32, pub bgra: Vec<u8> }` (premultiplied BGRA, top-down, ready for `shot_edit::gfx::bitmap`).
  - `pub enum Job { Page { page: u32, width_px: u32, gen: u64 }, Thumb { page: u32, width_px: u32 } }`
  - `pub enum Done { Page { page: u32, gen: u64, px: Pixels }, Thumb { page: u32, px: Pixels }, Failed { page: u32 } }`
  - `pub struct Renderer` with `pub fn start_pdf(bytes: Vec<u8>, password: Option<String>, notify: impl Fn() + Send + 'static) -> Result<Renderer, RenderOpenError>` (loads on the worker, blocking the caller only until load result: use a `mpsc::sync_channel(0)` handshake), `pub fn start_image(path: PathBuf, notify) -> Result<Renderer, RenderOpenError>`, `pub fn page_count(&self) -> u32`, `pub fn page_size_dip(&self, page) -> (f32, f32)` (the WinRT `Size`, already rotated, 96 DPI), `pub fn request(&self, job: Job)` (a newer `Page` job for the same page replaces a queued older one; Thumbs queue behind Pages), `pub fn take(&self) -> Vec<Done>`. Drop stops the thread (a `stop` flag + condvar) and joins it.
  - `pub enum RenderOpenError { Password, Broken, Unsupported }`: `0x8007052B` → Password, `0x80048018` / `0x8004808E` → Broken.
- **How:** one `std::thread` named `sign-render` with `RoInitialize(RO_INIT_MULTITHREADED)` and `CoInitializeEx(MULTITHREADED)`; calls exactly as in the spike: `InMemoryRandomAccessStream` + `DataWriter::CreateDataWriter`, `WriteBytes`, `StoreAsync()?.join()?`, `FlushAsync()?.join()?`, `DetachStream()`, `Seek(0)`; `PdfDocument::LoadFromStreamAsync` / `LoadFromStreamWithPasswordAsync(&ms, &HSTRING::from(pw))` `.join()`; `PdfPageRenderOptions::new()?.SetDestinationWidth(width_px)`; `RenderWithOptionsToStreamAsync(&out, &opts)?.join()`; read back with `DataReader`; decode the PNG with WIC to premultiplied BGRA. `.join()` only on this worker, never the UI thread. Images: WIC decode once (full size, cap 250 M px), downscale per job with WIC scaler.
- [ ] **Step 1: Failing tests:**
  - `renders_generated_pdf_page`: a 2-page lopdf PDF; `page_count == 2`; request Page 0 width 400 → within 10 s a `Done::Page` with `w == 400` and `h ≈ 517` (±2), some non-white pixel where the page draws a black rectangle.
  - `password_pdf_reports_password`: AES-128 generated → `start_pdf(bytes, None)` → `Err(Password)`; with `Some("rahasia")` → Ok.
  - `garbage_is_broken`: `b"%PDF-1.4 nonsense"` → `Err(Broken)`.
  - `newer_page_job_replaces_older`: queue Page 0 gen 1 and gen 2 before the worker runs (hold it with a test hook or a `paused` flag) → only gen 2 arrives.
  - `drop_joins_thread`: after drop, no thread named `sign-render` remains (track with an `AtomicUsize` counter incremented/decremented in the thread).
- [ ] **Step 2-4:** FAIL, implement, PASS + whole suite.
- [ ] **Step 5: Commit** `feat(sign): page renderer on Windows.Data.Pdf`.

---

### Task 6: Office conversion

**Files:**
- Create: `src/sign/office.rs`
- Test: in-file; live conversion tests are `#[ignore]` (run by hand on this PC, which has Microsoft Office 16; no WPS, no LibreOffice).

**Interfaces:**
- Produces:
  - `pub enum Suite { Microsoft, Wps, LibreOffice(PathBuf) }`
  - `pub enum App { Writer, Sheets, Slides }` and `pub fn app_for(path: &Path) -> Option<App>` (doc/docx → Writer, xls/xlsx → Sheets, ppt/pptx → Slides).
  - `pub fn detect(app: App) -> Option<Suite>`: Microsoft if `HKCR\<ProgID>\CLSID` exists (`Word.Application`, `Excel.Application`, `PowerPoint.Application`); else WPS (`KWps.Application` / `Kwps.Application`, `KET.Application` / `Ket.Application`, `KWPP.Application` / `Kwpp.Application`, first that exists); else LibreOffice: `HKLM\SOFTWARE\LibreOffice\UNO\InstallPath` + `\soffice.com`, then `App Paths\soffice.exe`, then `%ProgramFiles%\LibreOffice\program\soffice.com`, `%ProgramFiles(x86)%\…`.
  - `pub enum ConvertError { NoSuite, Locked, Failed, TimedOut }`
  - `pub fn convert(src: &Path, out_dir: &Path, cancel: &AtomicBool) -> Result<PathBuf, ConvertError>`: runs on the caller's thread (the window spawns a `sign-office` thread for it), timeout 90 s.
  - `pub fn temp_root() -> PathBuf` = `%TEMP%\Vale\sign`, `pub fn sweep(root: &Path)` (`remove_dir_all`, errors ignored), `pub fn job_dir() -> io::Result<PathBuf>` (`temp_root()\<pid>-<counter>`).
- **COM path (Microsoft and WPS):** `CoInitializeEx(APARTMENTTHREADED)` on the thread; `CLSIDFromProgID`; snapshot PIDs of `WINWORD.EXE` / `EXCEL.EXE` / `POWERPNT.EXE` (WPS: `wps.exe` / `et.exe` / `wpp.exe`) before `CoCreateInstance::<IDispatch>(CLSCTX_LOCAL_SERVER)`; after it, `ours = new PID appeared` (compare snapshots; for Word/Excel also `GetWindowThreadProcessId(Application.Hwnd)`). A small `Disp` helper: `get(name)`, `put(name, VARIANT)`, `call(name, &[VARIANT]) -> VARIANT` over `GetIDsOfNames` + `Invoke` (args reversed into `rgvarg`; skipped optionals `VT_ERROR`/`DISP_E_PARAMNOTFOUND`; property put with `DISPID_PROPERTYPUT`).
  - Writer: `Visible=false`, `DisplayAlerts=0`, `AutomationSecurity=3`; `Documents.Open(FileName, ConfirmConversions=false, ReadOnly=true, AddToRecentFiles=false, PasswordDocument="vale-no-password")`; `ExportAsFixedFormat(out, 17)`; `Close(0)`.
  - Sheets: `Visible=false`, `DisplayAlerts=false`, `AutomationSecurity=3`; `Workbooks.Open(Filename, UpdateLinks=0, ReadOnly=true, Format=missing, Password="vale-no-password")`; `ExportAsFixedFormat(0, out)`; `Close(false)`.
  - Slides: never set `Visible`; `Presentations.Open(FileName + "::vale-no-password", ReadOnly=-1, Untitled=0, WithWindow=0)`; zero slides → `Failed`; `SaveAs(out, 32)`; `Close()`.
  - A wrong-password COM error on Open → `Locked`. `Quit()` only if `ours`. Timeout: a watchdog thread waits 90 s or until done; on timeout set `cancel`, `TerminateProcess` the PID only if `ours`, return `TimedOut`.
- **LibreOffice path:** `soffice.com --headless --norestore --nolockcheck -env:UserInstallation=file:///<job_dir>/profile --convert-to pdf --outdir <job_dir> <src>` with `CREATE_NO_WINDOW`, 90 s timeout + kill; success only if `<job_dir>\<stem>.pdf` exists.
- [ ] **Step 1: Failing tests:**
  - `app_for_extensions`.
  - `quit_only_when_we_started_the_process`: pure function `should_quit(before: &[u32], after: &[u32], app_pid: Option<u32>) -> Option<u32>`: new PID present → `Some(pid)`; app PID in `before` → `None`; no new PID → `None`.
  - `soffice_args_use_private_profile`: `soffice_args(src, job)` contains `-env:UserInstallation=file:///…/profile` with forward slashes and `--convert-to pdf`.
  - `sweep_removes_folder`.
  - `#[ignore] live_word_to_pdf`, `#[ignore] live_excel_to_pdf`, `#[ignore] live_powerpoint_to_pdf`: generate a tiny file through the same COM (Word `Documents.Add` + `SaveAs2` docx) then convert; PDF exists and starts with `%PDF`. Also `#[ignore] live_word_keeps_users_instance`: start Word via COM with a document open and `Visible=true`, convert another file, assert the first instance's PID is still alive and its document count unchanged; then quit it.
- [ ] **Step 2-4:** FAIL, implement, PASS (+ run the `#[ignore]` ones once with `cargo test --locked -- --ignored sign::office` and paste the result into the commit message). Check afterwards that no `WINWORD/EXCEL/POWERPNT` process started by the tests is left (`Get-Process`), list any in the report.
- [ ] **Step 5: Commit** `feat(sign): convert Office files to PDF offline`.

---

### Task 7: Ink and the signature store

**Files:**
- Create: `src/sign/ink.rs`, `src/sign/store.rs`, `src/sign/raster.rs`
- Test: in-file

**Interfaces:**
- Produces (`raster.rs`, shared with Tasks 4 and 8):
  - `pub fn offscreen(w: u32, h: u32, draw: impl FnOnce(&ID2D1RenderTarget, &ID2D1Factory, &IDWriteFactory)) -> Option<Vec<u8>>`: D2D render target on a WIC bitmap (`GUID_WICPixelFormat32bppPBGRA`), transparent clear, runs `draw`, returns premultiplied BGRA top-down. COM must be initialised by the caller.
  - `pub fn to_mark(w: u32, h: u32, pbgra: &[u8]) -> Mark` (unpremultiply to straight RGBA).
  - `pub fn encode_png_rgba(m: &Mark) -> Option<Vec<u8>>` and `pub fn decode_png_rgba(bytes: &[u8]) -> Option<Mark>` (WIC, alpha kept).
- Produces (`ink.rs`):
  - `pub enum Ink { Blue, Black }` → `#1F3A8A`, `#1C1C1C`.
  - `pub fn from_strokes(strokes: &[Vec<(f32, f32)>], ink: Ink) -> Option<Mark>`: Catmull-Rom smoothing, stroke width 3.2 at a 300×90 logical pad rendered at 2× (600×180 px), antialiased with `raster::offscreen`, then `crop`. Empty / all-transparent → `None`.
  - `pub fn from_text(name: &str, ink: Ink) -> Option<Mark>`: DirectWrite system font "Ink Free", fallback "Segoe Script", fallback "Segoe Print", 64 px, then `crop`.
  - `pub fn from_photo(path: &Path, ink: Ink) -> Option<Mark>`: WIC decode (cap 40 M px, downscale long side to 1600), luminance `L`; alpha = 255 when `L < t_low`, 0 when `L > t_high`, linear between, with `t_low`/`t_high` from the image's own median (`t_high = median - 25`, `t_low = t_high - 60`, clamped), colour = ink; then `crop`. No ink found → `None`.
  - `pub fn crop(m: Mark, pad: u32) -> Option<Mark>`: bounding box of alpha > 8, plus `pad` px.
- Produces (`store.rs`):
  - `pub struct Saved { pub signatures: Vec<Mark>, pub initials: Vec<Mark> }` (each at most 4; newest first; adding a fifth drops the oldest).
  - `pub fn path() -> PathBuf` = `crate::data_dir::dir().join("signatures.bin")`.
  - `pub fn load_from(path: &Path) -> Saved` (empty on any error), `pub fn save_to(path: &Path, s: &Saved) -> io::Result<()>`: plain = a small binary (`b"VSIG1"`, counts, per mark `w, h, len, PNG bytes` via `raster::encode_png_rgba`), `dpapi::protect(&plain, w!("Vale signatures"))`, `dpapi::wipe(&mut plain)`, write `signatures.bin.tmp` then rename (pattern `clipboard/persist.rs:176`).
  - `pub fn add(s: &mut Saved, m: Mark, initials: bool)`, `pub fn remove(s: &mut Saved, initials: bool, i: usize)`.
- [ ] **Step 1: Failing tests:**
  - `strokes_make_a_cropped_mark`: one diagonal stroke → `Some`, `w < 600`, `h < 180`, some pixel with alpha 255 and RGB ≈ ink blue.
  - `empty_strokes_give_none`, `single_point_stroke_is_a_dot`.
  - `typed_name_uses_installed_font`: `from_text("Siti", Blue)` → `Some` (Ink Free ships with Windows 11).
  - `photo_white_background_becomes_transparent`: a generated 400×200 PNG (`phone::thumb::tests::write_gradient_png`-style helper, write your own: white background, a dark grey 8 px line) → corner alpha 0, line alpha > 200, mark smaller than 400×200.
  - `blank_photo_gives_none`.
  - `store_roundtrip_through_dpapi`: save 2 signatures + 1 initials to `temp_dir`, load → same sizes and pixels; the file bytes do not contain `VSIG1` (sealed).
  - `store_keeps_four_newest`.
  - `corrupt_store_loads_empty`.
- [ ] **Step 2-4:** FAIL, implement, PASS + whole suite.
- [ ] **Step 5: Commit** `feat(sign): signature ink and encrypted store`.

---

### Task 8: The signing window and the signature maker

Load `antislop:antislop`, `antislop:antislop-ui` and `antislop:antislop-human` before writing UI; check every new colour pair with the `check_contrast` tool (text ≥ 4.5:1, marks and focus ≥ 3:1) in both palettes.

**Files:**
- Create (replace the Task 1 stub): `src/sign/window/mod.rs` (registry, `open`, reports), `window.rs` (wndproc, input), `layout.rs` (pure layout + hit-test), `paint.rs`, `maker.rs` (signature maker popover), `text_edit.rs` (inline EDIT for Teks, copied from `shot_edit/text.rs`)
- Test: in-file (`layout.rs` pure tests; window built hidden with `ON_SCREEN`)

**Interfaces:**
- Consumes: everything from Tasks 1-7; `theme::current_window() -> WindowPalette`, `crate::ui::app_icon::class_icons`, `shot_edit::gfx::{bitmap, color, rect_f}` (call, don't edit).
- Produces (unchanged from the stub, now real): `open(owner: HWND, src: Source, demo: bool) -> bool` (same path already open → focus it, return true), `release()` (destroy all, join workers), `restyle_open()` (re-read the palette), `take_reports() -> Vec<Report>` (the window pushes `Report::Saved(path)` into a global `Mutex<Vec<Report>>` and posts `WM_APP_SIGN` to `owner`), `run_demo()`.
- **Window:** class `ValeSign`, `WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN`, 1100×760 dip centred on the monitor of `owner`, min 720×520, title = file name. Layout per mockup: title row (file name, "Halaman n dari N"), tool row (Tanda tangan, Paraf, Tanggal, Teks, Centang | Putar, Hapus halaman), rail 132 dip with thumbnails 74 dip wide, stage with one page fitted (max scale so the shown page fits both ways, 24 dip margin), footer (output name line, for images a Gambar/PDF segmented choice, Batal, Simpan). A banner row under the tools for `SignNoChanges`. Loading / converting / errors fill the stage with one line and, for errors, a Tutup button.
- **States:** `Converting` (Office) → `Password` (inline password EDIT with `ES_PASSWORD`, Buka button, wrong → `SignPasswordWrong`) → `Ready` → `Saving` → closes after a successful save. Error states: `SignCannotOpen`, `SignTooBig`, `SignNoOffice`, `SignOfficeFailed`, `SignOfficeLocked`, `SignCannotKeepLock`, `SignSaveFailed` (window stays open, nothing lost).
- **Interaction:** tool click then click on the page places the item centred on the pointer, upright (`geom::upright_turn`), default sizes: signature 150×50 pt scaled to the Mark's aspect, initials 48×24 pt, date 110×14 pt, text starts an inline EDIT, check 14×14 pt. Tanda tangan / Paraf with no saved mark open the maker first; with saved marks a small picker (saved tiles + "+ Baru"). Selected item: dashed accent outline, four corner handles (resize keeps aspect), floating bar (Lebih kecil, Lebih besar, + Tanggal under it for signatures, Pasang di semua halaman for initials, Hapus). Drag moves; arrows nudge 1 pt (Shift 10 pt); Delete removes; Ctrl+Z / Ctrl+Y; PgUp / PgDn and rail click change page; Ctrl+S saves; Esc deselects, else closes; mouse wheel at the page edge changes page. Teks text not encodable by `winansi::encode` is turned into a Mark with `ink::from_text`-like rendering in Arial (not the handwriting font) so the PDF writer only sees encodable text.
- **Save:** footer shows `names::output_path` (fallback wording when it falls back). Simpan: if `opened.signed && (has_deleted || has_rotated)` → confirm bar `SignBreaksSignature` + Simpan tetap / Batal; else save on a `sign-save` thread (`pdf_write::save` or `image_out`), write with `create_new` (copy `screenshot::win::write_at`), push `Report::Saved`, close. Close with changes → Discard / Keep bar (`Key::EditDiscard`, `Key::EditKeep`) like the screenshot editor.
- **Maker popover** (`maker.rs`, a child popup of the window, 400 dip wide): tabs Gambar / Ketik / Dari foto, pad 300×150 dip on paper colour `#FFFDF9` with a dashed baseline, Ulangi, Tinta swatches (blue, black, each with a light ring so the black one shows on dark), checkbox "Simpan di PC ini" (default on) with the hint, saved tiles (max 4 + "+ Baru"), Batal / Pakai. Dari foto opens the system file dialog (`IFileOpenDialog`, images only). Pen input: handle `WM_POINTER*` so pen and touch draw too.
- **Workers and idle:** the window owns its `Renderer` and any `sign-office` / `sign-save` threads; they end when it closes. Its only timer is a one-shot re-render debounce (id 1, 120 ms) after resize, on its own HWND. Office temp job dir removed on close.
- **Demo:** `run_demo()` writes a generated 3-page sample PDF (grey lines, heading "Contoh dokumen", the last page with two signature boxes) to `office::job_dir()`, opens it with `demo = true` (the store is in memory only, never `signatures.bin`), runs its own message loop, `PostQuitMessage` on destroy (pattern `shot_edit/window.rs:2246`).
- [ ] **Step 1: Failing tests** (`layout.rs`, pure):
  - `layout_fits_page_in_stage`: 1100×760 dip, page 612×792 → page rect inside the stage with ≥ 24 dip margin, aspect kept.
  - `rotated_page_fits_too`: shown size 792×612.
  - `hit_test_order`: handles before item body before page before stage; the floating bar wins over items under it.
  - `rail_scrolls_to_current_page`.
  - `narrow_window_keeps_tools_reachable`: at 720 dip the tool row overflows into a "…" menu instead of clipping.
- [ ] **Step 2: Failing test** (`window.rs`): `opens_hidden_and_closes`: `open(HWND::default(), demo source, true)` in a test builds the window without showing it (`ON_SCREEN` false), `release()` destroys it and no `sign-render` thread remains.
- [ ] **Step 3-4:** implement, run PASS + whole suite.
- [ ] **Step 5: Live check** (agent, by hand): `cargo run -- --demo-sign` (after Task 9 adds the flag; until then a temporary `#[test] #[ignore]` that calls `run_demo`): place a signature, initials on all pages, date, text with "Rp 1.500 – Bu Siti", a check, rotate page 2, delete page 3, save; open the output in Edge's PDF viewer and confirm everything is upright and in place. Screenshot the window in dark and light for the report.
- [ ] **Step 6: Commit** `feat(sign): signing window and signature maker`.

---

### Task 9: App wiring

**Files:**
- Create: `src/app_sign.rs`
- Modify: `src/app.rs` (`shelf_menu_target` ~8919, `RightClick` ~8904, dispatch ~10701, `open_shelf_card_menu` ~9678, `WM_APP_SIGN` handler next to `WM_APP_TRANSCRIPT` ~10420, `release_graphics` ~904, restyle calls at ~7095 and ~10385, startup next to `clip_start`), `src/ui/tray.rs` (new menu helper), `src/main.rs` (`--demo-sign` in `opens_windows` ~95-116 and the match ~186), `src/app_shelf.rs` only if a helper is needed for adding a path.
- Test: in-file in `src/app.rs` tests module and `src/ui/tray.rs`

**Interfaces:**
- Consumes: `sign::{is_signable, kind_of, Source}`, `sign::window::{open, release, restyle_open, take_reports, Report, WM_APP_SIGN, run_demo}`, `sign::office::{temp_root, sweep}`.
- Produces:
  - `pub fn card_menu(hwnd: HWND, labels: &[&str]) -> Option<usize>` in `src/ui/tray.rs` (multi-item, `TPM_RETURNCMD`, ids 1..=n). `note_card_menu` stays as it is.
  - `RightClick::ShelfMenu(usize)` now also returned for signable, non-stale cards on the Shelf page; `fn shelf_menu_items(card: &ShelfCard, path: &Path) -> Vec<ShelfAction>` with `enum ShelfAction { Sign, Preview }` (Sign first, then Preview for previewable images).
  - `impl App { pub(super) fn sign_open(&mut self, i: usize); pub(super) fn on_sign_reports(&mut self, now: Instant); pub(super) fn sign_start(&mut self) }`.
- **Behaviour:** pick Sign → `sign::window::open(hwnd, Source{path, kind}, false)` and collapse the panel like opening a card. `WM_APP_SIGN` → `on_sign_reports`: for each `Saved(path)`: notice `Notice { kind: NoticeKind::ShotSaved { path: Some(path.clone()) }, title: tr(SignDone), body: SignNoticeBody with the file name (shortened with `renderer.notice_text_fits` like `app_shot.rs:435`) }` via `self.apply(Event::Notice(n), now)`, then add the path to the shelf the way `shelf_add_phone` does (`self.shelf.shelf.add(vec![path])`, `shelf_look`, `shelf_animate`). `sign_start` sweeps `office::temp_root()` once at startup (call it next to `clip_start`). `release_graphics` calls `sign::window::release()`. The two restyle sites call `sign::window::restyle_open()`.
- [ ] **Step 1: Failing tests:**
  - Keep `a_right_click_on_an_image_card_gives_the_shelf_menu` passing.
  - Change `a_right_click_on_a_stale_card_a_document_the_wrong_page_or_nothing_gives_no_shelf_menu`: a `.pdf` Document card now gives `Some`; a `.txt` Document card, `Code`, `Folder`, stale cards, index 9, `ShelfClear`, `Notes`, `Idle` still give `None`. (The test helper `shelf_test_card` gains a path or the target takes paths; keep it minimal.)
  - `shelf_menu_items_order`: png → `[Sign, Preview]`; pdf → `[Sign]`; heic (previewable, not signable) → `[Preview]`; docx → `[Sign]`.
  - `sign_saved_report_makes_a_reveal_notice_and_a_shelf_card`: feed `Report::Saved("C:/x/A (ditandatangani).pdf")` → state has a notice whose click effect is `Effect::RevealFile(that path)` and the shelf contains the path.
  - `demo_sign_flag_opens_windows` in `src/main.rs` tests if `opens_windows` has tests; otherwise skip.
- [ ] **Step 2-4:** FAIL, implement, PASS + whole suite.
- [ ] **Step 5: Live check** (agent): `cargo run --release` (stop any installed Vale first: `Stop-Process -Name vale, dynamic-island`), drag a PDF, a JPG and a DOCX onto the notch, right-click each card → Tanda tangani, sign, save; notch notice appears and opens Explorer on click; the result is on the shelf. Check with `Get-Process` that no `sign-*` thread work or Office process is left after closing.
- [ ] **Step 6: Commit** `feat(sign): open signing from the shelf, notice and shelf card after saving`.

---

### Task 10: Size, notices, final verification

**Files:**
- Modify: `THIRD-PARTY-NOTICES.md`, this plan's Size log

- [ ] **Step 1:** `cargo build --release --locked`; size − baseline. ≤ 1,363,148 → record in the Size log. Over → stop, report the number and the biggest contributors (`cargo bloat --release --crates -n 20` if installed, else say so) to the owner.
- [ ] **Step 2:** `cargo tree -e normal --prefix none | Sort-Object -Unique` for the new crates; add lopdf (MIT) and each new crate's licence line to `THIRD-PARTY-NOTICES.md` in the file's existing format.
- [ ] **Step 3:** `cargo test --locked` → all pass; paste the summary line.
- [ ] **Step 4: Commit** `docs: third-party notices for lopdf; size log`.

## Size log

(Task 1 and Task 10 append here.)

- Task 1 baseline (main at 9271d1c, `cargo build --release --locked`): vale.exe = 5,009,920 bytes. Budget: at most 1,363,148 bytes more (6,373,068 total).
- Task 1: `miniz_oxide` bumped to 0.9 (matches lopdf's flate2); the whole suite passes unchanged (4747 passed), so 0.9 is kept.
