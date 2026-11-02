---
tags: [fonts, build, decision]
date: 2026-11-02
---

# Studio font sources and subsetting

Fonts are built by `scripts/make-fonts.ps1` and embedded as RCDATA 203-210: DM Sans 400/500/600 (203-205), DM Mono 400/500 (206-207) and JetBrains Mono 400/500/600 (208-210, the UI font option [[ui-font-option]]; see [[studio-font-loading]]).

- The italic serif display face of 3.0 (RCDATA 201-202) was dropped in v3.1 for upright DM Sans 600 titles and large clocks. Ids 201-202 stay unused.
- DM Sans comes from upstream `googlefonts/dm-fonts`, not `google/fonts`: the google/fonts build (4.004) has no `tnum` feature, so tabular figures would not work.
- DM Sans 400/500/600 and DM Mono 400/500 share name ID1 = family and ID2 = Regular, so the loader tells them apart by `usWeightClass`.
- **JetBrains Mono** comes from upstream `JetBrains/JetBrainsMono`: the static TTFs `fonts/ttf/JetBrainsMono-Regular.ttf`, `-Medium.ttf`, `-SemiBold.ttf` and `OFL.txt` at the commit of the latest release tag, v2.304 (`cd5227bd1f61dff3bbd6c814ceaf7ffd95e947d9`), each pinned by SHA-256 in `$Sources`. Same `set_names` contract (ID1/ID16 = "JetBrains Mono", ID2 = Regular, weight from `usWeightClass`) and the same `UNICODES` subset (348 mapped code points in it, more than DM Sans's 325: it has the check mark).
- **Ligatures are dropped in the subset.** The JetBrains Mono jobs pass a feature list without `calt`, `liga` and `dlig` (`FEATURES_NO_LIGA`: kern, ccmp, locl, mark, mkmk, tnum, pnum, lnum, those present), so "->", "!=", "==" and "www" read literally in DirectWrite and GDI alike. `VerifyPy` fails if a JetBrains Mono file has one of them in its GSUB, or if its digits 0-9 differ in advance; `font_coverage.rs` checks the GSUB again on the embedded bytes. After the subset the GSUB keeps only `ccmp` and `locl`.
- Sources are pinned by commit and SHA-256; output is byte-identical across runs (checked for the JetBrains Mono addition with two fresh `-WorkDir`s: identical SHA-256 for all eight files, and the DM Sans and DM Mono files identical to those of v3.12). Total font size is about 314 KB (budget 370 KB): DM 189 KB plus JetBrains Mono 125 KB (41.8, 41.5 and 41.9 KB), hinting kept.
- The JetBrains Mono OFL (Copyright 2020 The JetBrains Mono Project Authors) is appended to `assets/fonts/OFL.txt` through `$ofl`, and listed in `THIRD-PARTY-NOTICES.md`.
- The 3.0 screenshots in `assets/screenshots` still show the old italic titles; the v3.1 recapture (`src/ui/demo.rs`, `scripts/capture-screenshots.ps1`) replaces them. See [[demo-data-time-of-day]] before capturing.

Related: [[studio-palette-accent-aliases]], [[studio-tabular-figures-word-gaps]], [[exe-size]] (why the subset was not trimmed further; `src/ui/font_coverage.rs` guards it)

Part of [[v3-studio-overview]].
