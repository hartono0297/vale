---
tags: [decision, site, pages, design, a11y, susun]
date: 2026-11-01
---

# The landing page: direction A, minimal, with a working notch

`site/` is static (English, no build step, no framework, no library, no third-party request; fonts are local Geist / Geist Mono woff2, see [[github-pages]]) and published as is by [[github-pages]]. History: the first rebuild put a working notch on a light page; direction B ("dark stage") made it dark by default, minimal and centred. On 2026-11-01 the owner chose **direction A**: the same dark stage and working notch, but far less text, so a visitor grasps Vale at a glance: a hero, a grid of eight feature tiles, three screenshots, the Susun scrub, the cat, install, and the long facts (Google Calendar, privacy, uninstall) folded into native `<details>`. A fixed header with two floating pills flanks the live notch, and a cat-head link goes back to the top. The brand mark is still the terracotta cat head ([[logo]]).

Not used, on purpose: heavy cards, shadows on tiles, bento, a fake terminal window, eyebrow labels, uppercase tracked labels, arrows on buttons, stats, testimonials, an FAQ, em dashes (U+2014 appears nowhere under `site/`). Sentence case everywhere. The "New in 3.10" label and the five Susun facts (`ul.ar-facts`) are gone.

## Theme

Dark is the default whatever `prefers-color-scheme` says. `:root` holds the dark set (`color-scheme: dark`), `:root[data-theme="light"]` the light set; nothing reads the system scheme, in CSS or in `site.js`. There are now **two** `button.theme-btn` (one in the right pill, one in the Menu dropdown; only one is displayed at a time); `initTheme` paints and wires both: the label says the effective theme ("Switch to light theme" while dark), a click flips `data-theme` and stores `vale-theme` in try/catch. The inline head script is byte-identical on all four pages and applies a stored light choice before the first paint. `body` keeps an explicit background. Checked: with the system scheme emulated light and dark and nothing stored, the page is rgb(23, 18, 15), `color-scheme: dark`; a click sets and stores light, a second click dark.

## Tokens and measured contrast (WCAG ratios)

| Token | Dark (default) | Light |
|---|---|---|
| page | #17120F | #F6F6F4 |
| tint (tiles, Try the cat, scrolled pills, Menu dropdown, legal summary, table heads) | #1E1815 | #ECEBE7 |
| text | #F4EDE4 (16.00; 15.11 on the tint) | #17120F (17.18) |
| secondary | #A89A8B (6.78; 6.40 on the tint) | #5C524A (7.03; 6.38 on the tint) |
| terracotta ink (links, focus, tile icons, details marker) | #E2683C (5.56; 5.25 on the tint) | #A8431C (5.57; 5.05 on the tint) |
| code background | #221B17 | #EBEAE6 |
| control border (Copy, coat chips, dropdown theme button) | #8A7D70 | #7A6E65 |
| hairline (decoration: scrolled pill border, details rules) | #3A302A | #DCD8D3 |

Primary button and the header Download pill: #E2683C fill, #17120F text, 5.56 (hover #EC7C55), both themes; its focus ring is the text colour so it shows against the fill. Notch, same in both themes: surface #17120F, text #F4EDE4, secondary #A89A8B, accent #E2683C; 1 px edge #75675B in the dark theme (3.41 on the page), the surface colour in the light theme. The Susun drawing is a dark screen in both themes (screen #1E1815, edge #75675B, tile #2A221D, text #F4EDE4, numbers #A89A8B, current tile #E2683C).

**Go-to-top:** a #17120F disc (the notch surface) with the #75675B edge, the cat #E2683C with #17120F eyes. Dark: edge 3.41 on the page, cat 5.56 on the disc. Light: disc 17.18 on the page, cat 5.56 on the disc. Focus ring: 3 px ink, 5.56 (dark) and 5.57 (light) on the page.

**Sweep** (headless Chrome, reduced motion, every `<details>` opened, every visible element with its own text against its first opaque ancestor background): all four pages, dark and light, 1280 x 800 and 375 x 812: 0 failures (index 119 elements at 1280, privacy 257, terms 82, 404 17; lowest 5.05). The header text was swept separately at the top (transparent pills, on the page) and after scrolling (tinted pills), and the 375 Menu dropdown open: 0 failures, lowest 5.56.

## Section order (index.html)

hero (`data-notch="idle"`); `#features` (tiles, idle); `.f-music` (music); `.f-panel` (panel); `.f-clip` (clipboard); `#susun.susun` (arrange); `#cat` (cat); `#install` (idle); `#google-calendar` (idle); `#privacy` (idle); footer. Every section says `data-notch`.

- **Hero:** the cat logo (`img/logo.svg`, decorative), h1 "Your Windows, / with a notch." (two `span.l` blocks), one line "Free and open source, for Windows 11.", "Download for Windows" (`releases/latest/download/vale.exe`), the install line `irm .../install.ps1 | iex` as a `.cmd` with `code#cmd-hero` and a Copy button (`data-copy="#cmd-hero"`), and one js-only hint "The notch at the top is live. Try it."
- **Tiles** (`ul.tiles > li.tile.reveal`, 8): Music (`i-music`), Susun (`i-layout`), Clipboard (`i-clipboard`), Screenshot (`i-camera`), System (`i-chip`), Search (`i-search`), Calendar (`i-calendar`), File shelf (`i-file`); each an icon, an h3 and one short line. `i-camera` and `i-search` are new sprite symbols converted from `Icon::Camera` and `Icon::Search` in `src/ui/icons.rs` (24 grid, 1.5 round stroke, Camera with its 0.38 duotone fill; Search has no fill part). Tint background, 16 px radius, no border or shadow; 4 columns from 48em, 2 below (no overflow at 320). Then one quiet line: "Also in the panel: Pomodoro, Reminders, Notes, Notifications, Devices, Mixer, CLI and Shortcuts."
- **Three screenshots only:** music.png, notch.png, clipboard.png, each an h2 and a figure with one caption line, `width`/`height`, `loading="lazy"` and the same specific alt text as before.
- **Try the cat:** heading and one line; the coat and accessory radio pickers unchanged; the no-js line "In Vale the cat follows your pointer, in four coats and six accessories."
- **Install:** h2, one line, "Download vale.exe", the install line (`code#cmd-install`, Copy), then `<details>` "Update, uninstall and details" with the update/installer/facts line, Uninstall and the purge variant (`$env:DI_PURGE = '1'; ...`), each with Copy.
- **Google Calendar:** three visible sentences (optional; Google's own page; data only between the PC and Google; never deletes events), then `<details>` "How Vale uses Google Calendar" holding the previous facts word for word (OAuth with PKCE, the 127.0.0.1 loopback only during login, built-in client or your Client ID, list/create/edit and never delete, the read-only/recurring/multi-day limits, the 10-minute refetch, both scope URLs with their explanations, disconnecting and myaccount.google.com/permissions, the privacy policy link). Google OAuth verification wants these on the homepage; they are in the DOM and one click away.
- **Privacy in short:** two visible sentences that name the update check, the policy link, then `<details>` "Everything that can leave your PC" (the update check first, then LRCLIB, Google Calendar, web search, links, the installer; album covers).

`<details>` are native and open without script (click and Enter checked with script disabled); nothing in them hides behind `.js`.

**Removed** from index.html: the pair (Screenshot + File shelf), System, Search + CLI, Calendar, the Pomodoro/Reminders/Notes list and the Notifications/Devices list, with their paragraphs, the "Fourteen pages" page-name pills and the live-hint notes. **Removed from `site/img`** (grep first, nothing references them): calendar, cleaner, cli, clipboard-copied, editor, screenshot, search-files, shelf, system. `assets/` untouched. The kept panel shots still have the painted-over page-dot row (see Screenshots below).

## The update check (a fact the site had wrong)

Since v3.11 Vale has a self-update check ([[self-update]]), so "no update checker" was wrong. Release builds ask `https://api.github.com/repos/hartono0297/vale/releases/latest` about once a day while **Check for updates automatically** (Settings > About, on by default) is on, with a `User-Agent: Vale/<version>` header; clicking Update downloads `vale.exe` and `vale.exe.sha256` from the GitHub release (github.com, redirected to objects.githubusercontent.com) and checks the SHA-256; debug builds never check. The homepage privacy lead and list name it. privacy.html: the summary bullet and the `#network` paragraph no longer say "no update checker", the exceptions bullet no longer calls every exception opt-in, and the `#network` table has an "Update check" row (destination, what is sent, when, how to turn it off). Nothing else in privacy.html's `<main>` changed (diffed against the previous main: only those lines; "Last updated: 1 November 2026" kept); terms.html's `<main>` is identical. README.md ("Nothing is sent anywhere except ...") and the privacy list in docs/guide.md now name the update check too (Finish step, 2026-11-02).

## The header: two pills around the notch

Markup (identical on all four pages apart from the `index.html#` prefix): `header.site-header > div.nav-inner > div.pill.pill-l + div.pill.pill-r`. Left pill: `a.brand` (cat + "Vale", `index.html`), `button.menu-btn` (icon + "Menu", `aria-expanded`, `aria-controls="site-nav"`), `nav#site-nav` with `li.nav-more` Home, Features (`#features`), Install (`#install`), `li.nav-more` GitHub and `li.nav-more.nav-theme > button.theme-btn`. Right pill: `a.nav-gh` GitHub, `button.theme-btn`, `a.btn-dl` "Download" (`releases/latest/download/vale.exe`). "Try the cat", Privacy and Terms left the header; Privacy and Terms are in the footer.

- With script, `.site-header` is `position: fixed; top: 0`, full width, `pointer-events: none` (only the pills take events, so the notch between them stays reachable), `z-index` 90 under the notch's 100. Pills: 3 px padding, a 1 px border that is always there (transparent at first, so their size never changes), 44 px targets inside, 8 px from the top, so they end at y 60. `--edge` (the side gutter) is 16 px below 40em and 24 px above.
- **Scrolled state:** the scroll loop adds `.is-scrolled` once `scrollY > 12` (and `initHeader` sets it once at start, so a reload mid-page is right; checked: reload at 2000 is scrolled with the go-to-top shown). Then the pills get the tint and the hairline, a 300 ms background/border transition, none under reduced motion.
- **Below 49em** the left pill is only the Menu button and the right pill only Download; the nav opens under the Menu button as a dropdown (`.is-open`, tint, 22 px radius, `min(17rem, 100vw - 2 x edge)` wide): Home (cat + "Home"), Features, Install, GitHub, and the theme button. The first link gets focus on open; Escape closes and returns focus to the button; a click outside, a click on a link, Tab out of the dropdown, or crossing 49em closes it. Below 23.5em the Menu button is icon-only (the word stays as its accessible name); Download shrinks (14 px text, 10 px padding below 32.5em; 12 px and 7 px below 23.5em). Below 20em the word needs 312 px of media width beside the 96 px notch, so Download becomes a 44 px icon (CSS mask `--i-download`, the word moved out of view with `text-indent` so it stays the accessible name; with script only). Measured with the 15 px scrollbar: both gaps at least 12 px from 263 px up (23.5 px at 280). The Menu button has 8/10 px side padding and a 6 px gap.
- **Why 49em:** with both full pills (left about 250 px, right about 230 px) a 190 px idle notch only leaves 12 px each side from about 762 px of *client* width up. Gotcha: media queries count a classic scrollbar (15 px in Chrome on Windows, headless too), but the fixed header and the notch live in `clientWidth`, so the notch sits 7.5 px left of where the media width implies. Every tier is therefore budgeted with a scrollbar: 49em for the fold (48em left 6.7 px at 768), and the Menu and Download pills were trimmed for the 120 px (376 px) and 96 px (320 px) tiers rather than moving 23.5em past 390 px phones. Without script the 404's static notch is hidden below 49em, since the unfolded static header reaches under it.
- **Clearance (measured, with the 15 px scrollbar):** a sweep from 320 to 1440 px in 1 px steps at the page top (reduced motion) compared the idle notch box with both pill boxes: at least 12 px everywhere, closest 14.72 px at 784 (the left pill beside the 190 px notch); 404 the same with a forced scrollbar. A settled scroll scan (40 px steps) gave 14.7 at 784 x 800, 15.1 at 320 x 640, 42.6 at 375 x 812. Without a scrollbar every gap is 7.5 px larger.
- **Without script** the header is static, the pills wrap (two rows at 375), the dropdown-only items, the Menu and theme buttons are hidden, and Vale, Features, Install, GitHub and Download are visible and inside the viewport at 375 and 1280.
- `[id] { scroll-margin-top: 5rem }`, so anchored headings land 80 px from the top, below the pills.

## The notch vs the pills: measured room, not breakpoints

The old `.is-compact` (copy notice small "while the header is on screen") is gone: the header is always on screen now. Instead `pillFx` in the scroll loop reads both pill boxes in the read phase (on start, on resize, when fonts load, and on every frame while a wide state is wanted; `pillEls` is built once) and computes `room.music`, `room.panel`, `room.clipboard`: the state's box (mirrored sizes, centred, from the top) plus `PILL_MARGIN` 8 px on every side must not intersect either pill. In `refresh()`:

- **music** without room: `.is-tight`, the idle footprint (190 x 34; 120 x 45 / 96 x 45 on phones) with the cover and the EQ bars and no lyric line;
- **clipboard** (section state and the 3 s copy toast) without room: `.is-tight`, the compact idle-size form; the section state also goes tight when fitCheck finds text under it (the toast ignores text, as before). On phones the fixed header leaves no room, so the section state is always tight there. Gotcha: at 96 px only 42 px are left for the title (76 inside the padding, minus cat 26 and gap 8), and "Copied" is 47 px at 14 px, so it wrapped to "Copie / d"; `.clip-title` is `nowrap` and 12 px (41 px) below 23.5em;
- **panel** peek without room (or with text under it): the idle notch;
- **arrange** is now the idle footprint at every width (it was 200 wide on desktop), so it never needs room.

So at 1280 music, panel and clipboard show at full size; at 768 and 375 they never do. Checked with a real Copy click: at 1280 the toast is 340 x 56, at 375 96 x 45, 0 pill intersections, idle again after 3 s.

## The notch (unchanged apart from the above)

A fixed container with a `button.notch-face` (closed states, `aria-expanded`, `aria-controls="notch-panel"`) and `#notch-panel`. Sections carry `data-notch`; an IntersectionObserver on a thin band (rootMargin `-40% 0 -55% 0`) picks the last section in the band.

| State | Section | What it shows |
|---|---|---|
| idle | hero, tiles, install, Google Calendar, privacy | the cat left, the visitor's 24 h clock right (Geist Mono 400) |
| arrange | `.susun`, only while its stage is pinned and the scrub is on | the Layout icon and "Tile n of 3", then "Arranged"; idle footprint |
| music | `.f-music` | cover, a demo lyric line following the section's progress, 5 EQ bars; `.is-tight` without room |
| panel | `.f-panel` | the peek (title, one line, the 14-icon dock row), 400 x 128 (64 high on phones), only with room and no text under it |
| clipboard | `.f-clip`, and 3 s after any Copy button | "Copied" with a preview; `.is-tight` without room or (section state) over text |
| cat | `#cat` | the cat following the pointer, in the chosen coat and accessory |

The **pomodoro section state is gone** (no section asks for it any more; its `.nf-pomo` markup and CSS were removed). The Pomodoro **dock page** stays. Everything else is as before: the 120 ms hover-rest open (4 px still radius), 160 ms leave grace, click pins, touch taps, keyboard (Enter/Space opens and focuses the selected tab with `preventScroll`, dock scrolled after the width `transitionend`, Escape returns focus), the **14-tab dock** in `Page::ALL` order with roving tabindex and the 14 demo pages, `tabs.length` never hard-coded, Dock magnification `1 + 0.9 * falloff(d)` with R = 2 x pitch eased with tau 70 ms, and the cat's look (head 17.23, tilt 6, eyes 9.85, tau 120 ms).

## fitCheck (the peek must not cover text)

Unchanged rule: the panel peek and the clipboard section state show at full size only while the band under the notch (height + 16 px, width + 8 px each side) holds no text element; shrinking is immediate, growing waits 150 ms. The header is now excluded from the text list (the pills are handled by the room rule).

**Scan** (2026-11-01, headless Chrome 156 through the DevTools protocol, light theme stored, 40 px steps, 200 ms wait, then until the notch has no running animation and the same box 120 ms apart). Every settled closed box is compared with both pill boxes; panel or clipboard taller than 45 px, and arrange, are also compared with every text element:

| Viewport | Steps | Settled states | Pill hits | Text hits |
|---|---|---|---|---|
| 1280 x 800 | 167 | idle 88, music 300x36 15, panel 400x128 5, clipboard 340x56 7, clipboard tight 7, arrange 190x34 26, cat 19 | 0 | 0 |
| 768 x 800 | 156 | idle 84, music tight 12, clipboard tight 12, arrange 26, cat 22 | 0 | 0 |
| 375 x 812 | 146 | idle 96x45 89, music tight 9, clipboard tight 10, arrange 14, cat 24 | 0 | 0 |

Controls (a patched copy of `site/` served on another port): with `roomFor` returning true, 768 x 800 has 50 pill hits (first: music 300 x 36 at scroll 1040) and 375 x 812 has 30; with `bandBlocked` returning false, 1280 x 800 has 13 text hits (first: the panel peek at 1840). So the scan sees both problems.

## Susun section and its sticky scrub (behaviour unchanged)

Markup: `section#susun.susun > .ar-scrub > .ar-stage > .wrap.ar-grid` (copy left, drawing right from 52em, stacked below); the copy is one h2 ("Susun puts your windows in place.") and one line. The illustration is unchanged (six layout minis, the Main + 2 screen, `role="img"` with its aria-label, "An illustration, not a screenshot."). The default is the final state (no script, reduced motion, a stage that does not fit).

- **Scrub:** `.is-scrub` when motion is welcome and the stage content plus padding fits 100svh (measured with a hidden fixed probe; again on `document.fonts.ready` and `loadingdone`, since Geist's metrics differ from the fallback; and on viewport size changes, never on `innerHeight`). Then `.ar-scrub` is 230vh (170vh below 32.5em) and `.ar-stage` sticky, 100svh. `p = clamp(-outer.top / (outer.height - svh), 0, 1)`, steps 0 to 3 at 0.25, 0.5, 0.75, backwards on the way up. Only opacity and transform move.
- **Fit** (grid height / with padding): 1280 x 800 511 / 623, 768 x 800 579 / 691, 375 x 667 and 375 x 812 489 / 577, 320 x 640 517 / 605: all scrub now (320 x 640 did not before; the copy is shorter). Stage top padding 5rem (80 px), 4.5rem (72 px) below 32.5em, so the pinned stage clears the pills (end at 60) and the notch (34 / 45): pinned, the first content sits at 168 (1280 x 800), 135 (768), 189 (375 x 812), 117 (375 x 667), 90 (320 x 640).
- **Scroll position** handling (1)-(5) is kept as is: (1) the first choice is made synchronously when `site.js` runs, before the browser handles the fragment; (2) until the reader scrolls or presses something, and while the target still sits where the fragment scroll put it, a `location.hash` target after the wrapper is put back with `scrollIntoView({ block: 'start', behavior: 'instant' })`; (3) otherwise, with the wrapper above the view, the first element after it is measured before and after the switch and only the remaining shift is scrolled (scroll anchoring may already have done it); (4) a resize flushes the effect so the switch happens at the resize; (5) `data-step` is set in the same task as the class. Re-checked fragment loads after this redesign (smooth scrolling on): `index.html#features`, `#susun`, `#cat`, `#install`, `#google-calendar`, `#privacy`, the privacy.html header link to `#install` and `privacy.html#network` land 80 px from the top (their scroll margin) at 1280 x 800 and 375 x 812; the one exception, `#privacy` at 375, sits at 130 because the page ends.

## Go to top

`<span id="top" tabindex="-1">` is the first element in `body` (before `a.skip`); `a.to-top[href="#top"][aria-label="Back to top"]` with the inline cat-head svg (`aria-hidden`) comes after the footer, identical on all four pages. Fixed bottom-right, 16 px from the corner below 40em, 24 px above; 48 x 48 px.

- **Without script:** always visible, a plain fragment link (checked: a click goes to `#top`, scroll 0, on all four pages at 1280 and 375).
- **With script** (`initToTop`, one effect in the scroll loop): hidden (opacity 0, `visibility: hidden`, so not focusable; `pointer-events: none`) until the hero's bottom is above the viewport, or after one viewport height on pages without a hero; hidden again near the top. It also hides while its box intersects a `.btn` (Download, Copy), a coat or accessory label or a footer link (the list is built once). `.reveal-off` (the 4 s failsafe) shows it. On click or Enter: `preventDefault`, `scrollTo(0)` (smooth only when motion is welcome, else instant), then `#top.focus({ preventScroll: true })`; the URL gets no `#top`.
- **Keyboard (checked):** from the last footer link Tab reaches it; Enter scrolls to 0 (after 1.6 s with motion, at once with reduced motion), focus is `#top`, the hash stays empty, the next Tab is the skip link and the one after the brand. index, privacy and terms; on privacy and terms it is hidden at 0.5 viewport and shown at 1.2. The 404 page is shorter than two viewports, so it never shows there (by the rule).
- The footer's bottom padding (6rem) keeps its links above the button at the end of the page.

## Scroll effects (one loop)

One passive scroll listener (plus resize and load) asks for at most one `requestAnimationFrame`; in it every effect that is on reads first, then all write. Effects: the hero (scale to 0.92, fade to 0.2, both ways), the Susun scrub, the Music lyric, fitCheck, `pillFx` (pill room), the header's `.is-scrolled`, and the go-to-top visibility. `fx.near` / `fx.flush` as before; no per-frame `querySelectorAll`; no wheel or touch interception, no scroll-snap, no smooth-scroll library. **Reveal** (h2, figure and `li.tile`; never the Susun stage): once, by IntersectionObserver; tiles stagger by 60 ms within a row.

## The shared markup contract (index, 404, privacy, terms stay in step)

Every `<head>`, in order: charset, viewport, title, description, `link rel=icon`, the inline script (identical everywhere), `link rel=preload` of `fonts/Geist-Variable.woff2`, `style.css`, `<script src="site.js" defer>`. The body starts with `span#top`, `a.skip`, then the header above, `main#main`, `footer.site-footer` (unchanged: brand, MIT licence, Privacy, Terms, GitHub, Issues, the not-affiliated note), then `a.to-top`. On `index.html` the Features and Install hrefs are `#features` and `#install`; elsewhere `index.html#...`. With script, `.legal` gets `4.5rem` more top padding so the h1 starts below the fixed pills. Legal pages otherwise as before: `main#main.legal > article.legal-doc`, ids unchanged, tables in `div.table-wrap[role=region][tabindex=0]`. `site.js` runs on every page; each part no-ops without its elements and runs in `safe()`: theme, menu, copy, cats, the loop, the notch, the hero, the header, go-to-top, Susun, reveal; `ready` is set last. The head script adds `html.js`, removes it at `DOMContentLoaded` unless `ready`, adds `reveal-off` after 4 s, drops `js` at 20 s.

## Without script, and reduced motion

Without script (checked at 1280 and 375 on all four pages): the header static and wrapped, its five links visible and inside the viewport; no Menu, theme or Copy button, notch or hint; 0 `.reveal` hidden; Susun static at its final state (wrapper = stage, 623 px at 1280, 577 at 375); the three `<details>` open by click and by Enter; the go-to-top link works; no overflow.

Reduced motion (emulated, checked): hero at scroll 400 `transform: none`, opacity 1, `will-change: auto`; no `.is-scrub` (wrapper = stage, no `data-step`), notch idle through Susun; 15 of 15 `.reveal` visible with no transform and a 0 s transition; pill, go-to-top and notch transitions 0 s; `scroll-behavior: auto`; go-to-top instant.

## The check script

`python -I scripts/check-site.py` (standard library only, exit 1 on any problem): parsing and nesting, unique ids, every internal href/src/CSS `url()` exists with exact case and every `#fragment` exists, tables in `div.table-wrap`, no third-party resource, the same inline head script and `style.css`/`site.js` everywhere, header and footer identical apart from the `index.html#` prefix and `aria-current`, the 14 dock tabs of `Page::ALL`, no U+2014 under `site/`. Changed or new in direction A: the header must link Features to `(index.html)#features`, Install to `(index.html)#install`, GitHub, and Download to `releases/latest/download/vale.exe` ("Try the cat" no longer required); the footer must link `privacy.html` and `terms.html`; `.reveal` only on h2, h3, figure and `li.tile`; every page has id `top` and an `a.to-top[href="#top"]` with an `aria-label`, the same markup everywhere; every font preload is a `.woff2` that an `@font-face` in `style.css` uses. Each new check was proven on a scratch copy with one injected fault at a time (no to-top, a to-top without a label, no `#top`, a preload of a woff2 no `@font-face` uses, a preload of a non-woff2, a footer without Terms, a header without Download, Install pointing at `#cat`, `.reveal` on a `p` and on a non-tile `li`): each one failed with its own message; the clean copy passed.

## Screenshots

From `assets/screenshots` into `site/img`: music, notch, clipboard (the others were removed with their sections). They were last regenerated on 2026-10-29, before Devices and Notifications existed, so in the `site/img` copies of the panel shots the old page-dot row is painted over with #17120F; `assets/screenshots` keeps the originals. When `scripts/capture-screenshots.ps1` ([[screenshot-capture-script]]) is re-run, copy the new files over and drop this note. There is no Susun screenshot; the Susun picture says it is an illustration.

## Verified, and not

Against `python -m http.server --directory site --bind 127.0.0.1` in headless Chrome 156 through the DevTools protocol (small Node scripts, cache disabled):

- **Layout:** index, privacy, terms and 404 at 1280 x 800 and 375 x 812, dark and light: `scrollWidth <= clientWidth` on all 16, 0 console errors or exceptions, every request to the site's own origin, Geist (100-900) and Geist Mono 400 loaded from `fonts/` as woff2 (status 200). Index at 320 x 640, both themes: no overflow, 0 pill/notch overlaps (gaps 44 and 13.8 px).
- **Screenshots looked at:** the hero at 1280 dark, 375 light, 320 dark; the navbar unscrolled and scrolled (1280); the tiles (1280 dark, 375 light); Music, the panel and Clipboard sections; Susun pinned (1280 dark, 375 light); Try the cat (375); Install (1280); the go-to-top button at the page end; the 375 Menu dropdown; privacy at 375 light, 404 at 1280 dark; the no-script hero at 375.
- **Keyboard, menu, theme, no-script, reduced motion, notch scans, fit, fragments, contrast, the check script:** as described in the sections above.
- `python -I scripts/check-site.py`: 0 problems. `cmp site/img/logo.svg assets/logo.svg`: identical. No U+2014 under `site/`. `cargo test --locked`: 1857 passed, 6 ignored, no warnings; no Rust change.

Not verified: a real phone (no touch events were generated), Safari and Firefox (`100svh`, `behavior: 'instant'`, sticky, `:is()`, the variable woff2 were only run in Chrome), a screen reader, scroll smoothness on a real mid-range laptop, the svh probe against a real moving toolbar, the live GitHub Pages site (the font MIME type there is assumed `font/woff2`).

Related: [[arrange]], [[panel-pages]], [[cat-colours-and-surfaces]], [[cat-accessories]], [[notch-peek-eyes]], [[cat-looks-at-pointer]], [[eq-follow-audio]], [[readme-structure]], [[studio-bata-logo-sources]], [[logo]], [[github-pages]], [[lyrics-sync]], [[self-update]], [[screenshot-capture-script]]
