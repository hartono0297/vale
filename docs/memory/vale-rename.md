---
tags: [decision, rename, install]
date: 2026-11-01
---

# The app is Vale now; what still says Dynamic Island, and why

Run 12 of the v3.1 plan. The display name, exe (`vale.exe`), install folder
(`%LOCALAPPDATA%\Programs\Vale`), data folder (`%APPDATA%\Vale`), pipe
(`\\.\pipe\vale`), Run value, window classes, tray tooltip and log prefix all
say Vale.

- **Data moves on first start** (`src/data_dir.rs`): rename `DynamicIsland` to
  `Vale`; if that fails (a file held open), copy into `Vale.migrating` and
  rename that; a note `MOVED-TO-Vale.txt` is left in the old folder. If the
  copy fails too, the half copy is removed and the app keeps using the old
  folder (`dir()` picks the old one while the new one is missing).
- **Old names kept on purpose**: the `Local\DynamicIsland` mutex is claimed next
  to `Local\Vale`, so an old and a new build never run together; Claude hooks
  that call `dynamic-island.exe --hook` are recognised, repointed and removed
  like Vale's; the old `DynamicIsland` Run value is always deleted at start.
- **install.ps1 / uninstall.ps1** stop both process names and remove the old
  program folder, Start Menu shortcut and Run value. Data stays unless
  `DI_PURGE=1` (the variable kept its name).
- **Sparse package**: Identity and application id are `Vale`, but the
  publisher stays `CN=DynamicIslandDev`, because it must match the dev
  certificate the owner already trusts (re-trusting needs admin).
  `package.ps1` removes an old `DynamicIsland` registration.
- **Repo renamed.** The GitHub repo was renamed to `hartono0297/vale` on 2026-11-01, and every repo URL in code, scripts, docs and site/ now uses it. Pages moved to https://hartono0297.github.io/vale/. GitHub redirects the old git, web, API, release and raw URLs, but not Pages. The old name `hartono0297/dynamic-island` must never be reused for a new repo, or those redirects break (old installs' self-updater and old one-liners rely on them).
- `scripts/make-fonts.ps1` keeps the `dynamic-island-subset` tag in the font
  names, since changing it changes the font files for no visible gain.

- Verify pass: an empty `%APPDATA%\Vale` (any app may make one) is removed and
  the data moves in; a non-empty one is still taken as Vale's. The old
  `Run\DynamicIsland` value is deleted only when it starts
  `dynamic-island.exe`.

- Settings → Tentang names the data folder in use (`data_dir::label`): the
  old one while a failed migration keeps it.
- Dynamic Island's `settings.json.bak-dynamic-island` (the true original)
  becomes `settings.json.bak-vale` on the next hooks install.

Related: [[v3-1-handoff]], [[v3-studio-overview]], [[install-script-follows-latest-release]]
