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

## The phone link, phase 1 (2026-11-04)

[[shelf-phone]] (the local HTTP server, the page, the QR code, the four views
and their drawing) took the release exe from 3,186,688 to 3,299,840 bytes
(+113,152, +3.6 %), measured at the branch's last code commit (an earlier
count, 3,295,232, was taken before the review fixes). Built with
`cargo build --release --locked` in two folders of the same name length
(`main` and `phon`, both from `git archive`), each with its own
`CARGO_TARGET_DIR`, so the running island's exe was never touched; no
warning in either. The same branch built in the longer worktree path came
out 512 bytes smaller (the embedded PDB path), so only same-length folders
compare. The new parts: `qrcodegen` 1.8.0 (one crate, no
dependency), four `windows` features (`Win32_NetworkManagement_IpHelper`,
`Win32_NetworkManagement_Ndis`, `Win32_Networking_WinSock`,
`Win32_Networking_NetworkListManager`), about 14 KB of page text, the
Indonesian and English strings and the layout and paint code. No HTTP crate
and no thread pool.

## Idle RAM and CPU

Not measured. The release exe is single-instance and reads real user data, and
the owner's Vale must keep running; the debug `--demo` process says little
about release. The changes here (a smaller icon resource) do not touch runtime
code. Idle budget stays as in [[bata-idle-power-budget]].

Listed in [[index]].
