---
tags: [gotcha, install, release]
date: 2026-10-28
---

# install.ps1 is live at once, but installs the latest release

`irm https://raw.githubusercontent.com/hartono0297/vale/main/install.ps1 | iex`
runs the script from `main` (live a few minutes after a push; the raw URL
is cached up to about 5 minutes) but downloads the exe from the latest GitHub
*release*. The two can disagree.

What happened (2026-10-28): the rename pushed an `install.ps1` that looked
for `vale.exe` while the latest release was still v3.0.0 with
`dynamic-island.exe`. Every install failed until a fix was pushed and the
v3.1.0 release existed.

- `install.ps1` looks for `vale.exe` and falls back to `dynamic-island.exe`
  (the pre-3.1 names). Keep a fallback like that whenever asset names change.
- When the script on `main` depends on something new in the release, push the
  tag right after (or before) it.
- Check without installing: run the script's lookup, download and checksum
  steps against `releases/latest` into a temp folder.

Related: [[vale-rename]], [[v3-1-handoff]]
