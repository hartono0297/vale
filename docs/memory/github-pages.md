---
tags: [decision, ci, pages]
date: 2026-11-01
---

# GitHub Pages from site/

GitHub Pages can only serve the repo root or `docs/` from a branch, not a folder named `site/`. So `.github/workflows/pages.yml` uses the GitHub Actions source: it uploads `site/` as the artifact and deploys it, with no build step. The owner sets Settings -> Pages -> Source: GitHub Actions once. The workflow runs on pushes to `main` that touch `site/**` or the workflow file, and by hand.

## What site/ holds (since the 2026-10-31 rebuild)

`index.html`, `404.html`, `privacy.html`, `terms.html`, one `style.css`, one `site.js`, `fonts/` (DM Sans 400/500/600, DM Mono 400/500 and `OFL.txt`, copied from `assets/fonts`) and `img/` (`logo.svg` plus the screenshots the page uses). English only. No build step, no framework, no library.

**Zero third-party requests.** No Google Fonts, no CDN, no analytics: the fonts are the same subset TTFs the app embeds, loaded by `@font-face` with `font-display: swap`, and only DM Sans 400 and 600 are preloaded. Checked in the browser's network list: every request goes to the site's own origin.

Before pushing a site change, run `python -I scripts/check-site.py` (links, ids, the shared header and footer, no em dash; the workflow does not run it). How the page is built, the notch, the shared markup contract and the colours are in [[site-landing]]. The logo file stays byte-identical to `assets/logo.svg` ([[logo]]).

`404.html` uses relative asset paths like the other pages. On a project site (`user.github.io/dynamic-island/`) GitHub serves it for any missing URL, so on a nested missing path (`/dynamic-island/a/b`) the stylesheet and links resolve against that path and break: the page then shows as plain HTML with working-looking but wrong links. Accepted: a base-path guess in script would not run before the CSS loads, and the site has no nested pages, so only a mistyped deep URL shows it.

Related: [[install-script-follows-latest-release]]
