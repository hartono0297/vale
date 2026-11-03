---
tags: [decision, ci, pages]
date: 2026-11-01
---

# GitHub Pages from site/

GitHub Pages can only serve the repo root or `docs/` from a branch, not a folder named `site/`. So `.github/workflows/pages.yml` uses the GitHub Actions source: it uploads `site/` as the artifact and deploys it, with no build step. The owner sets Settings -> Pages -> Source: GitHub Actions once. The workflow runs on pushes to `main` that touch `site/**` or the workflow file, and by hand.

## What site/ holds (since the 2026-10-31 rebuild)

`index.html`, `404.html`, `privacy.html`, `terms.html`, one `style.css`, one `site.js`, `fonts/` and `img/` (only `logo.svg`: the three screenshots, music, notch and clipboard, left with the 2026-11-03 "Night mix" redesign, which draws everything in CSS and SVG). English only. No build step, no framework, no library.

**Fonts (since 2026-11-01, direction A):** the site uses **Geist** and **Geist Mono**, not the app's DM Sans / DM Mono (`assets/fonts` and `scripts/make-fonts.ps1` are the app's and stay as they were). `site/fonts/` holds `Geist-Variable.woff2` (69,760 bytes, weights 100-900; smaller than the three statics 400/500/600 together, 137,996 bytes), `GeistMono-Regular.woff2` (50,764 bytes, the only mono weight: every mono use is weight 400, so nothing is synthesised) and `OFL.txt` (SIL OFL 1.1, © The Geist Project Authors). They come unchanged from the official repo `github.com/vercel/geist-font` at tag v1.9.0 (commit 72f67167dd30), files `fonts/Geist/webfonts/Geist[wght].woff2` and `fonts/GeistMono/webfonts/GeistMono-Regular.woff2`; the repo's `OFL.txt` was checked to be the OFL 1.1 first. Credited in `THIRD-PARTY-NOTICES.md` under Fonts, "Website fonts". `@font-face` uses `format("woff2")` and `font-display: swap`, with a system-ui fallback stack. Every page preloads only `fonts/Geist-Variable.woff2` (`type="font/woff2" crossorigin`); `scripts/check-site.py` fails if a font preload is not a `.woff2` used by an `@font-face` in `style.css`. Python's `http.server` on Windows serves `.woff2` as `application/octet-stream`; Chrome loads it anyway, and GitHub Pages sends `font/woff2`.

**Zero third-party requests.** No Google Fonts, no CDN, no analytics. Checked in the browser's network list on all four pages, both themes, 1280 and 375 wide: every request goes to the site's own origin, and the two woff2 files load from `fonts/` with status 200.

Before pushing a site change, run `python -I scripts/check-site.py` (links, ids, the shared header and footer, the go-to-top link, font preloads, no em dash; the workflow does not run it). How the page is built, the notch, the shared markup contract and the colours are in [[site-landing]]. The logo file stays byte-identical to `assets/logo.svg` ([[logo]]).

`404.html` uses relative asset paths like the other pages. On a project site (`user.github.io/vale/`) GitHub serves it for any missing URL, so on a nested missing path (`/vale/a/b`) the stylesheet and links resolve against that path and break: the page then shows as plain HTML with working-looking but wrong links. Accepted: a base-path guess in script would not run before the CSS loads, and the site has no nested pages, so only a mistyped deep URL shows it.

The live site is https://hartono0297.github.io/vale/ (moved from /dynamic-island/ with the repo rename on 2026-11-01; Pages URLs are not redirected).

Related: [[install-script-follows-latest-release]]
