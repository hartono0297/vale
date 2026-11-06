---
tags: [decision, build, size]
date: 2026-11-02
---

# Exe size: what was measured, what was trimmed

Measured with `cargo build --release --locked` (profile: `opt-level = "s"`,
lto, 1 codegen unit, `panic = abort`, strip). The shipped v3.5.0 exe is
2,479,104 bytes; the same source built in this worktree is 2,479,616 (the
embedded path of the PDB moves it by up to 512 bytes, so compare builds made
in folders of the same name length).

## Where the bytes are (2,441,728 after the icon change)

| Section | Bytes | What |
|---|---|---|
| `.text` | 1,909,760 | code: `vale` 1,000 KiB, `std` 572 KiB, serde_core/serde_json/chrono ~40 KiB each, hashbrown 32, `windows` 17 |
| `.rdata` | 275,456 | strings (i18n), tables |
| `.rsrc` | 208,384 | fonts 189 KB ([[studio-font-sources]]), icon 12.5 KB, manifest |
| `.pdata` | 37,376 | unwind tables |

`cargo bloat` (installed with `cargo install cargo-bloat`, needs
`CARGO_PROFILE_RELEASE_STRIP=false` and its own `CARGO_TARGET_DIR` to see
symbols): the biggest functions are `Renderer::paint_page` 55 KiB,
`app::run` 49, `App::apply` 30, `Renderer::page_layout` 27, `mole::serve` 26,
`std` process spawn 23. 3,000+ functions under 8 KiB make up 70 % of `.text`:
no single thing to cut.

## Tried

| Change | Exe bytes | Kept |
|---|---|---|
| baseline (v3.5.0 source) | 2,479,616 | |
| `opt-level = "z"` for everything | 2,153,472 (-326,144, -13 %) | **no**, see below |
| dependencies only at `z`, `vale` at `s` | 2,484,736 (+5 KB) | no |
| icon: small sizes as PNG frames, identical pixels | 2,441,728 (-37,888) | **yes** |
| `windows` features: drop `Win32_System_Com_StructuredStorage`, `Win32_System_WinRT`, `Foundation_Collections`, `ApplicationModel` (the only ones with no direct path in `src`) | did not compile (`STGMEDIUM`/`IDataObject::GetData`, `UserNotification::AppInfo`, screenshot frames need them) | no |
| fonts | untouched | see below |

- **`z` is an owner option, not applied.** It saves about 326 KB (about 2.12 MB
  with the new icon, derived not built) but the island
  animates the panel and the notch with per-frame drawing code, and no release
  demo can run inside the workflow to show the frame times are equal. Try it
  by hand (`opt-level = "z"` in `[profile.release]`), watch the page slides
  and the lyric scroll, then decide.
  **Decided 2026-10-30: the owner keeps `"s"`.** Don't propose `z` again
  unless size becomes a real problem.
- **Icon.** `scripts/make-icon.ps1` now writes PNG payloads at every size (it
  wrote 32 bpp DIBs below 128 px). Same nine sizes, 50,457 -> 12,573 bytes.
  `src/ui/icon_frames.rs` decodes every frame through WIC and compares an
  FNV-1a hash of the BGRA pixels with the hashes taken from the v3.5.0 icon, and
  loads the embedded icon with `LoadIconWithScaleDown` at 10 sizes. The
  regenerated `logo-256.png` came out byte-identical, so the script is
  deterministic.
- **Fonts left alone.** The subset is already ASCII, Latin-1, Latin Extended-A
  and the symbols; by table, `glyf` is 21-27 KB of each 35-39 KB file, the
  rest is hinting, kerning (`GPOS` 12.7 KB in DM Sans) and a 0.9 KB `name`
  table the loader reads. Dropping layout features (`liga`, `calt`, `ccmp`) or
  hinting changes rendering; nothing else is worth the risk. New guard
  `src/ui/font_coverage.rs`: parses the `cmap` of the embedded TTFs, pins the
  count of mapped code points in the subset per font (DM Sans 325, DM Mono
  316), checks the symbols the UI draws (`·`, `×`, `÷`, `°`, `±`, `§`, `–`,
  `—`, `…`, `›`, `→`, `↑`, `↓`, `−`, quotes, `•`, `€`, `™`, `%`) and scans every
  non-test, non-comment character in `src/**` against DM Sans. "Non-test"
  means each `#[cfg(test)]` item is cut out by brace depth (not the file from
  the first `#[cfg(test)]` on: i18n.rs and mole.rs have test-only helpers near
  the top), and files declared `#[cfg(test)] mod x;` are skipped. Characters
  matched but never drawn (U+201B in `ps_quote`) go in `NOT_DRAWN`.
  Characters already outside the fonts, drawn through DirectWrite's system
  fallback today, are listed in `SYSTEM_FALLBACK` (`↗`, `↳`, `≤`, `▸`, `♪`,
  box drawing, `✓`, `●`). DM Sans has no `✓` and DM Mono has no arrows; that
  is how the source families are, not a loss from subsetting.
- **No dead code** to remove: the release and test builds are warning-free
  (rustc's `dead_code` covers a binary crate), and nothing else is proven
  unused by grep.
- **Not tried on purpose:** UPX, removing features, font swaps, dependency swaps
  (`serde_json/preserve_order` pulls `indexmap`, 5 KiB), static CRT (changes
  the redistributable story), `build-std` (needs nightly).

## Equalizer capture (2026-10-31)

[[eq-follow-audio]] added about 31 KB (2,508,288 -> 2,539,008 bytes, +1.2 %):
the analyzer, the WASAPI client and the Settings row. No new crate and no new
`windows` feature.

## The typing cat (2026-11-01)

[[cat-types-while-working]] (the laptop, paws, glance, typing shape and
`TIMER_TYPING`) took the release exe from 2,816,000 to 2,821,120 bytes
(+5,120, +0.18 %). Both were built with `cargo build --release --locked` in
two folders of the same name length (`main` from `git archive`, and the
branch), each with its own `CARGO_TARGET_DIR`, so the running island's exe was
never touched. No new crate and no new `windows` feature.

## JetBrains Mono for the UI font option (2026-11-02)

[[ui-font-option]] embeds JetBrains Mono 400/500/600 (RCDATA 208-210). Built
with `cargo build --release --locked` in two folders of the same name length
(`main` from `git archive`, and the branch), each with its own
`CARGO_TARGET_DIR`, so the running island's exe was never touched:

| | Exe bytes |
|---|---|
| `main` (v3.12.0) | 2,913,792 |
| branch | 3,040,768 (+126,976, +4.4 %) |

Fonts total (`assets/fonts/*.ttf`): 321,880 bytes, 314.3 KB, up from 193,740
(189 KB): DM Sans 3 x 39.3 KB, DM Mono 35.3 + 35.9 KB, JetBrains Mono 41.8 +
41.5 + 41.9 KB. The budget is 370 KB (`LIMIT` in `VerifyPy`), so JetBrains
Mono keeps its hinting; if a later font does not fit, drop hinting for that
family only before touching the limit. The ligatures (`calt`, `liga`, `dlig`)
are already dropped from the subset ([[studio-font-sources]]). No new crate
and no new `windows` feature; the code added is the font selection, the
Mono sizes and the Settings row.

## The lyrics card (2026-11-04)

[[lyrics-card]] (its window, pure layout and colour modules, the sixth hotkey row,
the Musik button, `MediaCommand::Seek` and `can_seek`) took the release exe from
3,186,688 to 3,233,280 bytes (+46,592, +1.46 %). Built with `cargo build
--release --locked` from `git archive` of `main` and from the branch, each in its
own `CARGO_TARGET_DIR` (folder names of different length, so up to 512 bytes of
the difference is the PDB path). No new crate and no new `windows` feature.

## The phone link, phase 1 (2026-11-04)

[[shelf-phone]] (the local HTTP server, the page, the QR code, the four views
and their drawing, the video icon, the format-control and UTF-16 name rules)
took the release exe from 3,235,328 to 3,351,040 bytes (+115,712, +3.6 %),
measured on the fix-up branch's last code commit merged onto `main` f83dbf0
(which already has the lyrics card; the first count, 3,186,688 to 3,299,840,
was made before it and before the review fixes). Built with
`cargo build --release --locked` in two folders of the same path length
(`base` and `head`, both from `git archive`), each with its own
`CARGO_TARGET_DIR`, so the running island's exe was never touched; no
warning in either. A folder name of a different length moves the exe by up
to 512 bytes (the embedded PDB path), so only same-length folders compare.
The new parts: `qrcodegen` 1.8.0 (one crate, no
dependency), four `windows` features (`Win32_NetworkManagement_IpHelper`,
`Win32_NetworkManagement_Ndis`, `Win32_Networking_WinSock`,
`Win32_Networking_NetworkListManager`), about 14 KB of page text, the
Indonesian and English strings and the layout and paint code. No HTTP crate
and no thread pool.

## The phone link, phase 2 (2026-11-04)

[[shelf-phone]] phase 2 (shared set, Range, WIC thumbnails, the file routes, the page's
second tab, the send view and its strings) took the release exe from 3,463,168 to
3,505,664 bytes (+42,496, +1.23 %). Built with `cargo build --release --locked` from
`git archive` of `main` (c1fa7a7) and of the branch head, in two folders of the same path
length (`base` and `head`), each with its own `CARGO_TARGET_DIR`; no warning in either. No
new crate and no new `windows` feature (WIC was already enabled); the page grew from
14 KB to 24 KB of text.

## Idle RAM and CPU

Not measured. The release exe is single-instance and reads real user data, and
the owner's Vale must keep running; the debug `--demo` process says little
about release. The changes here (a smaller icon resource) do not touch runtime
code. Idle budget stays as in [[bata-idle-power-budget]].

Listed in [[index]].

## Desktop widgets, phase 1 (2026-11-04)

`cargo build --release --locked`, same toolchain, before (main at 3.17.0) and
after:

| Build | Bytes |
|---|---|
| `main` (3.17.0) | 3,186,688 |
| branch (widgets phase 1) | 3,294,720 (+108,032, +3.4 %) |

No new crate and no new `windows` feature: the host, the drawing, the
checklist, snap and placement maths, the colour popover, the Settings page and
the extra strings. `--demo-widgets` is debug-only and not in the release exe.

After the finish merge with the lyrics card (`cargo build --release --locked`, 0 warnings) `vale.exe` is 3,346,944 bytes; the part from the lyrics card is on [[lyrics-card]].

## The phone link, simplified (v3.19, 2026-11-04)

[[shelf-phone]] lost the approval (code, pairing cookie, rejections), the Kirim ke HP
tick view and its drawing, and gained the Public warning, the shelf sync with its loop
guard and the token's auto-rotation. The release exe went from 3,510,272 (`main`
81babbc, v3.18.0) to 3,488,256 bytes (-22,016, -0.63 %). Built with `cargo build
--release --locked` in two folders of the same path length (`base` and `head`, both
from `git archive`), each with its own `CARGO_TARGET_DIR`, so the running island's exe was
never touched; no warning in either. No new crate and no new `windows` feature.

## The sticky note list (2026-11-05)

[[sticky-note-list]]: the list window, its pure rules, the hidden-EDIT search,
the header's list button, the eighth hotkey row, the tray item, `NotesStore::restore`
and the strings (both languages). `cargo build --release --locked`, 0 warnings, the
same toolchain, built from `git archive` of the base and from this branch, each with
its own `CARGO_TARGET_DIR` (the two folders do not have the same path length, so the
embedded PDB path moves either exe by up to 512 bytes):

| Build | Bytes |
|---|---|
| `main` (9cfa869, 3.20.0 plus docs) | 3,632,128 |
| branch (sticky note list) | 3,686,400 (+54,272, +1.5 %) |

No new crate and no new `windows` feature (`MonitorFromPoint`, `GetDpiForMonitor`,
the EDIT and DirectWrite calls were already in use). `--demo-note-list` is debug-only
and not in the release exe.

## Format toolbar and images (2026-11-05)

[[widgets]], "Format toolbar and images": the rich-text runs, the bar and its popup,
bullets, up to four images (WIC import, decode, drawing), the clipboard format,
the phone page's formatting and the strings (both languages). `cargo build
--release --locked`, 0 warnings, built in this worktree (its path is not the same
length as the folders of the earlier entries, so the embedded PDB path moves the
exe by up to 512 bytes):

| Build | Bytes |
|---|---|
| sticky note list (entry above, 3.20.0 plus the list) | 3,686,400 |
| this branch (format toolbar and images) | 3,810,816 (+124,416, +3.4 %) |

No new crate and no new `windows` feature (WIC, `IFileOpenDialog`, the clipboard,
`DragQueryFileW` and Direct2D bitmap brushes were all in use already). The demo's
two gradient PNGs are generated at run time in debug builds only: no image is
bundled, so the exe grew by code and strings alone.

## Dirty-rect present and the EQ skip (2026-11-06)

[[bata-idle-power-budget]], "Dirty-rect present and the EQ skip": `ui/dirty.rs` (pure
plan, bounds and `EqKey`, with its tests), the renderer's dirty clip and
`UpdateLayeredWindowIndirect` present, the app's `full_redraw` flags and
`draw_eq_tick`. The self-check (`DI_DIRTY_CHECK`) and the demo's `DI_HALO` are
debug-only. `cargo build --release --locked`, 0 warnings. Both exes were built
from clean copies of the sources in scratchpad folders (the embedded PDB path
moves either exe by up to 512 bytes), `main` at 996b646 (5.0.0) against this
branch:

| Build | Bytes |
|---|---|
| `main` (996b646, 5.0.0) | 3,830,784 |
| this branch (dirty rect and EQ skip) | 3,841,536 (+10,752, +0.3 %) |

No new crate and no new `windows` feature (`UpdateLayeredWindowIndirect` and
`UPDATELAYEREDWINDOWINFO` are in `Win32_UI_WindowsAndMessaging`, already on). The
growth is the planner, the clip and the key; the self-check compiles out.

## Island on a WIC bitmap target (2026-11-06)

[[bata-idle-power-budget]], "The WIC bitmap target", and [[d2d-dc-target-full-copy]]: the island
draws on a persistent software WIC bitmap target and copies the dirty rows into
the DIB, no `BindDC` per frame. `cargo build --release --locked`, 0 warnings. Both
exes were built from clean `git archive` copies of the sources in two scratchpad
folders of the same path length (the embedded PDB path moves an exe by up to 512
bytes), `main` at fcf5dd7 against this branch:

| Build | Bytes |
|---|---|
| `main` (fcf5dd7) | 3,851,264 |
| this branch (WIC target) | 3,852,800 (+1,536, +0.04 %) |

No new crate and no new `windows` feature (WIC and Direct2D were on already).
