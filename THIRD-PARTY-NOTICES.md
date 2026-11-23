# Third-party notices

Vale is © 2026 hartono0297 under the [MIT licence](LICENSE). It is built with,
and talks to, the work of others listed here. Each keeps its own licence;
thank you to all of them.

## In the app

### Mole for Windows (tw93)

[github.com/tw93/Mole](https://github.com/tw93/Mole/tree/windows), branch `windows`.

- **The cleaner's location list** (*Pick yourself* mode): which cache, temp
  and download folders are safe to clean, and after how many days, follows
  Mole for Windows 1.30.0, commit `500dfc4c`, © 2025 tw93, **MIT licence**.
  Only that data is used; the scanning and deleting code is Vale's own
  (`src/clean.rs`). The full licence text is in [`NOTICE`](NOTICE).
- **The *Automatic · Mole* mode** runs the copy of Mole that *you* installed, as a
  separate program, and reads what it prints. Mole is not bundled, copied or
  linked. Its `windows` branch is **GPL-3.0** since 2026-10-11; nothing from
  it after commit `500dfc4c` is used in Vale.

### Fonts

Latin subsets embedded in `vale.exe`, all under the **SIL Open Font Licence
1.1** (text and copyright lines in [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt)):

- **DM Sans** by Colophon Foundry for Google Fonts, © 2014 The DM Sans
  Project Authors ([github.com/googlefonts/dm-fonts](https://github.com/googlefonts/dm-fonts)).
- **DM Mono** by Colophon Foundry for Google Fonts, © 2020 The DM Mono
  Project Authors ([github.com/googlefonts/dm-fonts](https://github.com/googlefonts/dm-fonts)).
- **JetBrains Mono** (UI font option), Copyright 2020 The JetBrains Mono Project
  Authors ([github.com/JetBrains/JetBrainsMono](https://github.com/JetBrains/JetBrainsMono)),
  SIL Open Font Licence 1.1; the programming ligatures are dropped in the subset.
- Four handwriting fonts for typed signatures (renamed **Vale Sign Slant**,
  **Vale Sign Fine**, **Vale Sign Print** and **Vale Sign Formal**, because a
  subset is a Modified Version under the OFL and two of the sources carry
  Reserved Font Names), all from [github.com/google/fonts](https://github.com/google/fonts):
  - Vale Sign Slant is based on **Dancing Script** by The Dancing Script
    Project Authors, Copyright 2016 The Dancing Script Project Authors
    ([github.com/googlefonts/DancingScript](https://github.com/googlefonts/DancingScript)).
  - Vale Sign Fine is based on **Sacramento** by Brian J. Bonislawsky DBA
    Astigmatic (AOETI), Copyright (c) 2012 Brian J. Bonislawsky DBA Astigmatic (AOETI).
  - Vale Sign Print is based on **Kalam** by Indian Type Foundry,
    Copyright (c) 2014 Indian Type Foundry.
  - Vale Sign Formal is based on **Great Vibes** by The Great Vibes Pro
    Project Authors, Copyright 2015 The Great Vibes Pro Project Authors
    ([github.com/googlefonts/great-vibes](https://github.com/googlefonts/great-vibes)).

`scripts/make-fonts.ps1` rebuilds the subsets from the pinned upstream files.

**Website fonts.** The site in `site/` (not the app) uses **Geist** and
**Geist Mono**, © The Geist Project Authors
([github.com/vercel/geist-font](https://github.com/vercel/geist-font)), under
the **SIL Open Font Licence 1.1**; the text is in
[`site/fonts/OFL.txt`](site/fonts/OFL.txt).

### Rust crates

Every crate Vale is built from, from `Cargo.lock` (regenerate with
`scripts/third-party.py`). All are permissive (MIT, Apache-2.0, Zlib,
Unlicense, Unicode-3.0, BSD-3-Clause, 0BSD); their licence texts and
copyright lines are in each crate's repository and on crates.io.

### Dependencies of vale.exe (with the macros that generate its code)

| Crate | Version | Licence | Source |
|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 |
| aes | 0.9.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-ciphers |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause | https://github.com/dropbox/rust-alloc-no-stdlib |
| alloc-stdlib | 0.2.4 | BSD-3-Clause | https://github.com/dropbox/rust-alloc-no-stdlib |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| block-padding | 0.4.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| brotli-decompressor | 5.0.3 | BSD-3-Clause/MIT | https://github.com/dropbox/rust-brotli-decompressor |
| cbc | 0.2.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-modes |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/stream-ciphers |
| chrono | 0.4.45 | MIT OR Apache-2.0 | https://github.com/chronotope/chrono |
| cipher | 0.5.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| const-oid | 0.10.3 | Apache-2.0 OR MIT | https://github.com/RustCrypto/formats |
| core_detect | 1.0.0 | MIT/Apache-2.0 | https://github.com/thomcc/core_detect |
| cpubits | 0.1.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.5 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| digest | 0.11.3 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| ecb | 0.2.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-modes |
| encoding_rs | 0.8.44 | (Apache-2.0 OR MIT) AND BSD-3-Clause | https://github.com/hsivonen/encoding_rs |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| flate2 | 1.1.11 | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs |
| foldhash | 0.2.0 | Zlib | https://github.com/orlp/foldhash |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hybrid-array |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| inout | 0.2.6 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| itoa | 1.0.18 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| log | 0.4.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/log |
| lopdf | 0.47.0 | MIT | https://github.com/J-F-Liu/lopdf.git |
| md-5 | 0.11.0 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT | https://github.com/hsivonen/multiversion_no_op |
| nom | 8.0.0 | MIT | https://github.com/rust-bakery/nom |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| qrcodegen | 1.8.0 | MIT | https://github.com/nayuki/QR-Code-generator |
| quote | 1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| rand | 0.10.4 | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | https://github.com/rust-random/rand_core |
| rangemap | 1.8.0 | MIT/Apache-2.0 | https://github.com/jeffparsons/rangemap |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard |
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| simd-adler32 | 0.3.10 | MIT | https://github.com/mcountryman/simd-adler32 |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | https://github.com/rusticstuff/simdutf8 |
| stringprep | 0.1.5 | MIT/Apache-2.0 | https://github.com/sfackler/rust-stringprep |
| syn | 2.0.119 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| syn | 3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/tinyvec |
| typenum | 1.20.1 | MIT OR Apache-2.0 | https://github.com/paholg/typenum |
| unicode-bidi | 0.3.18 | MIT OR Apache-2.0 | https://github.com/servo/unicode-bidi |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| unicode-normalization | 0.1.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/unicode-rs/unicode-normalization |
| unicode-properties | 0.1.5 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/unicode-rs/unicode-properties |
| weezl | 0.2.1 | MIT OR Apache-2.0 | https://github.com/image-rs/weezl |
| windows | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| zlib-rs | 0.6.8 | Zlib | https://github.com/trifectatechfoundation/zlib-rs |
| zmij | 1.0.23 | MIT | https://github.com/dtolnay/zmij |

### Used only to build it (resources, the Windows SDK lookup)

| Crate | Version | Licence | Source |
|---|---|---|---|
| autocfg | 1.5.1 | Apache-2.0 OR MIT | https://github.com/cuviper/autocfg |
| cc | 1.10.0 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| embed-resource | 3.0.12 | MIT | https://github.com/nabijaczleweli/rust-embed-resource |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | https://github.com/djc/rustc-version-rs |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | https://github.com/dtolnay/rustversion |
| semver | 1.0.28 | MIT OR Apache-2.0 | https://github.com/dtolnay/semver |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| shlex | 2.0.1 | MIT OR Apache-2.0 | https://github.com/comex/rust-shlex |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| vswhom | 0.1.0 | MIT | https://github.com/nabijaczleweli/vswhom.rs |
| vswhom-sys | 0.1.3 | MIT | https://github.com/nabijaczleweli/vswhom-sys.rs |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| winnow | 1.0.4 | MIT | https://github.com/winnow-rs/winnow |
| winreg | 0.56.0 | MIT | https://github.com/gentoo90/winreg-rs |

The `windows` crates are Microsoft's Rust bindings for the Windows API
([github.com/microsoft/windows-rs](https://github.com/microsoft/windows-rs)).

**lopdf** ([github.com/J-F-Liu/lopdf](https://github.com/J-F-Liu/lopdf), **MIT**)
reads and writes the PDF files in the signing module (*Tanda tangan*), with the
PDF encryption, compression and parsing crates it pulls in, listed above.

## Services it talks to

Only when you use the feature, and only what it needs:

- **Google Calendar API** (Calendar page): your events, through Google's
  OAuth sign-in. [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy).
- **[LRCLIB](https://lrclib.net)** (synced lyrics, when turned on): the
  artist, title and length of the playing track. A free, open lyrics database.
- **Google Search** (Search, "search the web"): opens your query in the
  browser.
- **GitHub Releases** (`install.ps1`): downloads `vale.exe` and its checksum.

## Works with

- **[Claude Code](https://claude.com/claude-code)** by Anthropic: Vale adds
  hooks to `~/.claude/settings.json` (only when you click *Install hooks*) to
  show "Claude finished" and "Claude needs you".
- **Windows**: notifications, media sessions, audio, Explorer and the shell
  through the public Windows APIs.

## Trademarks

The notch idea is inspired by Apple's Dynamic Island. Vale is an independent
project, not affiliated with or endorsed by Apple, Microsoft, Google,
Anthropic, Spotify or any other company named here; their names and
trademarks belong to their owners.
