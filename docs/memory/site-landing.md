---
tags: [decision, site, pages, design, a11y, susun]
date: 2026-11-01
---

# The landing page: direction B, a dark stage with a working notch

`site/` is static (English, no build step, no framework, no library, no third-party request; fonts are the local DM Sans / DM Mono TTFs) and published as is by [[github-pages]]. The first rebuild (direction "A") put a real, working notch at the top edge of a light page. The owner then chose direction **"B: dark stage"**: the same working notch, but the page is dark by default, minimal and centred, so the notch (a dark surface) reads as part of the screen and the screenshots (dark panels) sit on their own colour instead of floating on a light page. One large DM Sans headline per section, tight tracking, lots of empty space, no decoration. The brand mark is still the terracotta cat head ([[logo]]).

Not used, on purpose: a card grid, bento, a fake terminal window, eyebrow labels, uppercase tracked labels, middle-dot meta strings, arrows on buttons, stats, testimonials, an FAQ, em dashes (U+2014 appears nowhere under `site/`). The one small label allowed is the DM Mono "New in 3.10" above the Susun headline.

## Theme

Dark is the default whatever `prefers-color-scheme` says. `:root` holds the dark set (`color-scheme: dark`), `:root[data-theme="light"]` the light set (`color-scheme: light`); nothing reads the system scheme any more, in CSS or in `site.js`. The theme button says the effective theme is `data-theme` if set, else dark; a click flips it and stores `vale-theme` in try/catch (it still toggles for the visit without storage). The inline head script is unchanged (byte-identical on all four pages): it reads `vale-theme` into `data-theme`, so a stored light choice applies before the first paint. `body` keeps an explicit background.

## Tokens and measured contrast (WCAG ratios, same formula as the antislop checker)

| Token | Dark (default) | Light |
|---|---|---|
| page | #17120F | #F6F6F4 |
| tint band (Try the cat, legal summary, table heads) | #1E1815 | #ECEBE7 |
| text | #F4EDE4 (16.00; 15.11 on the band) | #17120F (17.18) |
| secondary | #A89A8B (6.78; 6.40 on the band) | #5C524A (7.03; 6.38 on the band) |
| terracotta ink (links, focus, "New in 3.10", the chosen layout mini) | #E2683C (5.56; 5.25 on the band) | #A8431C (5.57; 5.05 on the band) |
| code background | #221B17 (text 14.61, ink 5.08, secondary 6.19) | #EBEAE6 (text 15.44, ink 5.01) |
| control border, outline button, Menu and theme buttons (3:1) | #8A7D70 (4.64; 4.39 on the band) | #7A6E65 (4.57; 4.15 on the band) |
| hairline (decoration only) | #3A302A | #DCD8D3 |

Primary button: #E2683C fill, #17120F text, 5.56 (hover #EC7C55, 6.71), both themes. Notch, same in both themes: surface #17120F, text #F4EDE4 (16.00), secondary #A89A8B (6.78), accent #E2683C (5.56); in the dark theme the notch is the page colour, so it has a 1 px edge #75675B (3.41 on the page); in the light theme the edge is the surface colour.

The Susun drawing is a dark screen in both themes: screen #1E1815 with an edge #75675B (3.41 on the dark page, 5.04 on the light page), filled tile #2A221D with the app name #F4EDE4 (13.44), tile numbers #A89A8B on the screen (6.40), the current tile's border #E2683C (5.25 on the screen), empty tile hairline #4A3F37 and placeholder bars #3A302A (decoration).

DOM sweep (every visible text node against its effective background, headless Chrome, reduced motion so nothing is mid-fade): dark and light, index 170 elements, privacy 248, terms 84, 404 19, no pair under 4.5:1 (3:1 for large text). The same sweep on index with the Susun scrub at step 1 (notch "Tile 2 of 3", the pick line shown): 147 elements, 0 failures, both themes.

## Section order (index.html)

hero; Susun (`#features`, new); Music; Fourteen pages in one panel; Clipboard; Screenshot + File shelf; System (with the cleaner); Search + CLI; Calendar; Pomodoro, Reminders and Notes; Notifications and Devices; Try the cat (`#cat`); Install (`#install`); Google Calendar (`#google-calendar`); Privacy in short (`#privacy`); footer. Every section says `data-notch` explicitly.

- **Hero:** centred h1 "A notch for the top of your Windows screen.", one line under it, "Download for Windows" (same href as before: `releases/latest/download/vale.exe`) and a quiet outline "View on GitHub", the js-only hint. About a viewport tall; the idle notch sits in the header row, far above the headline. The PowerShell line moved to Install, and so did the "For Windows 11 x64..." facts line.
- **Major features keep real screenshots** (from `site/img`, `width`/`height`, `loading="lazy"`, specific alt, at or below natural width): Music (music), panel (notch, the Calendar week page), Clipboard (clipboard, clipboard-copied), Screenshot and File shelf (screenshot, shelf, two columns with their own h2), System (system, cleaner), Search and CLI (search-files, cli, two columns), Calendar (calendar, editor). Centred heading and text, screenshots centred below.
- **Smaller features are a compact quiet list** (`dl.small-list`, name and one line each, hairlines, no cards): Pomodoro, Reminders, Notes (the section keeps `data-notch="pomodoro"`), then Notifications and Devices.
- **Try the cat** is unchanged in behaviour; the old "A cat that tells you things" section was folded into it as one paragraph (the notices fact). Google Calendar and Privacy in short keep their facts word for word (Google OAuth verification needs them on the homepage).
- **Removed from `site/img`** because nothing references them any more: pomodoro.png, reminders.png, notes.png, robot.png, greeting.png (`assets/` untouched). The ten panel shots still have the painted-over page-dot row described below.

## Susun section and its sticky scrub

Facts only from [[arrange]]: Susun ("Arrange" in the English interface) arranges open windows into a tile layout; six built-ins (2 columns, 3 columns, Main + 2, Grid 2x2, 2/3 + 1/3, Main + 3) or your own made in Settings; Vale asks for one tile at a time ("Tile 1 of 3"), you click a window in the list or on screen, with skip and back; it moves and resizes them into the work area with a gap of 0 to 24 px (8 by default); Undo in one click; save an arrangement and replay it later, apps that are not open are skipped, never launched; Ctrl+Alt+S, off by default. There is no Susun screenshot, so the stage holds an **obvious illustration**: six outlined layout minis (sprite symbols `l-cols2` ... `l-main3`, drawn from `BuiltIn::node`, Main + 2 in the ink colour), a dark screen with the Main + 2 layout (main 2/3, two rows on the right), generic app names Browser, Editor, Terminal with placeholder bars, a status line, and the caption "An illustration, not a screenshot." The drawing is one `role="img"` with an aria-label. "Undo" is a quiet text label inside the picture, not a button.

Markup: `section#features.susun > .ar-scrub > .ar-stage > .wrap.ar-grid` (copy left, drawing right from 52em, stacked below), then `ul.ar-facts` (five facts) in normal flow after the wrapper.

- **The default is the final state** (all three tiles filled, "Arranged, 3 windows. Undo" shown): that is what the page shows without script, with reduced motion, and when the stage does not fit the viewport.
- **Scrub:** `site.js` adds `.is-scrub` to the section when motion is welcome and the stage content plus its padding fits `innerHeight` (measured on load and whenever the viewport size changes). Then `.ar-scrub` is 230vh (100vh + 130vh; 170vh, that is + 70vh, below 32.5em) and `.ar-stage` is `position: sticky; top: 0; height: 100vh` (100svh where supported). Progress `p = clamp(-outer.top / (outer.height - innerHeight), 0, 1)`; the drawing gets `data-step` 0 (p < 0.25: tile 1 current), 1 (tile 1 filled, tile 2 current), 2 (tiles 1-2 filled, tile 3 current), 3 (p >= 0.75: all filled, notice shown). It runs backwards on the way up. Only opacity and transform move: a filled tile is an overlay `.ar-fill` that fades and rises in, the current tile's #E2683C border is an `::after` that fades. No ancestor of the stage has `overflow` other than visible.
- **Fit:** measured stage content (grid) heights: 1280x800 511 px, 1366x768 511, 1024x768 446, 768x800 647 (the stacked drawing is capped at 32rem so tablets scrub), 414x896 606, 375x667 581, 360x740 572; with the stage padding (80 + 32 px, 68 + 16 px on phones) all of these scrub. 320x640 does not and shows the final state.
- **Scroll position:** switching `.is-scrub` on changes the page height above everything after Susun, so when the wrapper is above the view at that moment (a load at `index.html#cat`, a restored scroll position) the script scrolls by the height difference with `behavior: 'instant'`, once.

## The notch

A fixed container with a `button.notch-face` (closed states, `aria-expanded`, `aria-controls="notch-panel"`) and `#notch-panel` (hidden when closed). Closed visuals are `aria-hidden`; scroll-driven changes are not announced. Sections carry `data-notch`; an IntersectionObserver on a thin band (rootMargin `-40% 0 -55% 0`) picks the last section in the band and keeps the previous state when none is in it.

| State | Section | What it shows |
|---|---|---|
| idle | hero and every section without its own state | the terracotta cat left, the visitor's 24 h clock right (DM Mono, one `setTimeout` aimed at the next minute, only while idle and the tab is visible) |
| arrange | `.susun`, **only while its stage is pinned** (`outer.top <= 0` and `outer.bottom >= innerHeight`) and the scrub is on | the Layout icon and "Tile n of 3", then "Arranged" at step 3 (crossfade via `data-ar` pick/done). Otherwise (not pinned, no scrub, reduced motion) the Susun section shows the idle notch |
| music | `.f-music` | a plain square cover, a demo lyric line, 5 EQ bars; the line follows the section's scroll progress; EQ is CSS keyframes, paused when the tab is hidden |
| panel | `.f-panel` | a "peek" (title, one demo line, the 14-icon dock row), 400 x 128, only while no text lies under it |
| clipboard | `.f-clip`, and 3 s after any Copy button | "Copied" with a preview; `.is-compact` while the header is on screen and `.is-tight` when it would cover text keep it at the idle size; the 3 s toast ignores `.is-tight` |
| pomodoro | `.f-pomodoro` | a ring at a fixed progress with a static 18:42 |
| cat | `#cat` | the cat, following the pointer, in the chosen coat and accessory |

Priority in `refresh()`: the copy toast, then arrange (when the scrub reports a pinned step), then the section's state, where "arrange" and a blocked "panel" become idle.

Sizes: idle and cat 190 x 34 (120 x 45 below 32.5em, so the face is a 44 px tap target; 96 x 45 below 23.5em), pomodoro and arrange 200 x 34 (same as idle below 32.5em; arrange drops the word "Tile" there, "2 of 3", and its icon below 23.5em), music 300 x 36 (300 x 45 on phones), clipboard 340 x 56, open 460 x 300 (viewport minus 32 px on phones). Width, height and radius ease in about 350 ms, content crossfades. Measured arrange text inside the notch: 1280 notch 540-740, text 623-683; 414 notch 147-267, text 192-248; 375 notch 140-236, text 159-216.

**Interaction** (unchanged from the first rebuild). Mouse hover opens the full panel only after the pointer rests on the notch for 120 ms (the shelf's `OPEN_DELAY_MS`; moving more than 4 px restarts the wait, leaving cancels it), because the idle notch overlaps the nav row. Leaving closes it (160 ms grace) unless pinned; a click pins, a second click closes. Touch: pointerenter is ignored, a tap toggles. Keyboard: the face is focusable, Enter or Space opens and focuses the selected tab with `preventScroll`; the dock is scrolled to the tab only after the width `transitionend`, never on `transitioncancel`. Escape closes and returns focus to the face; ArrowLeft/Right/Home/End move between the 14 icons (roving tabindex); Enter, Space or click selects. A click outside closes a pinned panel; Tab out of the panel closes it.

**Dock: 14 tabs** in `Page::ALL` order (Calendar, Pomodoro, Reminders, Notes, Music, Mixer, Clipboard, Notifications, File shelf, System, Devices, CLI, **Arrange**, Shortcuts) and 14 `role="tabpanel"` demo pages; `#tab-arrange` (aria-label "Arrange", `aria-controls="pg-arrange"`) sits after `#tab-cli`, and `#pg-arrange` (the six layout minis and "Pick a layout, then click a window for each tile.") between `#pg-cli` and `#pg-shortcuts`. Icons are the `Icon::*` paths of `src/ui/icons.rs` as `<symbol>`s (24 grid, 1.5 round stroke, duotone fill at 0.38); `i-layout` is `Icon::Layout`. Tabs are 30 px wide now (14 x 30 + 24 padding = 444 px inside the 458 px between the open notch's edges; at 32 px they would overflow). Magnification as in `src/ui/dock.rs`: scale `1 + 0.9 * falloff(d)`, `falloff = 0.5 * (1 + cos(pi d / R))`, `R = 2 x pitch`, eased with tau 70 ms, rAF only while moving. Coarse pointer or below 32.5em: no magnification, 44 x 44 tabs in a row that scrolls inside the panel (`overscroll-behavior: contain`). Nothing in `site.js` counts tabs; it uses `tabs.length`.

**The cat** (`#big-cat`, `#notch-cat`) is unchanged: geometry from `src/ui/robot.rs` and `cat_style.rs`, look = head slide 17.23 units, tilt 6 degrees, eye slide 9.85 units, eased with tau 120 ms; coat and accessory in native radio groups in `fieldset`/`legend`, read again on `pageshow`; greeting from the visitor's hour ([[greeting-periods]]).

## The peek must not cover text (fitCheck), now also for arrange

The `panel` peek (400 x 128, 64 high on phones) and the section's `clipboard` state (340 x 56) are shown only while the band under the notch (height + 16 px, width + 8 px each side) holds no text element (h1, h2, h3, p, li, dt, dd, figcaption, code, pre, label, legend, td, th, blockquote; `dt`/`dd` were added for the compact lists). Shrinking is immediate, growing waits 150 ms of free band. The arrange state needs no check of its own: it exists only while the stage is pinned, and the stage's top padding (5rem, 4.25rem on phones) is more than the notch height + 16 px at all three widths (34 + 16 at 200 px; 45 + 16 at 120 and 96 px).

Scan (2026-11-01, headless Chrome through the DevTools protocol, one page at a time, light theme stored in the test profile (the check is geometry, not colour); 40 px steps, 200 ms wait, then until the notch has no running animation and the same box 120 ms apart; then the box is compared with every text element when the state is panel or clipboard and taller than 45 px, or the state is arrange):

| Viewport | Steps | Settled and compared | Intersections |
|---|---|---|---|
| 1280 x 800 | 287 | arrange 200x34: 26, panel 400x128: 6, clipboard 340x56: 11 | 0 |
| 768 x 800 | 334 | arrange 200x34: 26, panel 400x128: 5, clipboard 340x56: 12 | 0 |
| 414 x 896 | 341 | arrange 120x45: 15, panel 382x64: 4, clipboard 340x56: 4 | 0 |
| 375 x 812 | 339 | arrange 96x45: 14, panel 343x64: 3, clipboard 340x56: 5 | 0 |

Control: with `bandBlocked` patched to return false (the response of `site.js` rewritten through the protocol's Fetch domain), the 1280 x 800 scan reports 86 intersections (the Music figcaption under the peek, first at scroll 3720), so the scan does see the problem. The cost is as before: on wide screens the peek mostly shows in the gaps between sections.

## Scroll effects (one loop)

`site.js` has one passive scroll listener (plus resize and load) that asks for at most one `requestAnimationFrame`. In that frame every effect that is on reads first (rects, sizes) and then all write (classes, transform, opacity, the notch state). An effect is on while an IntersectionObserver says its element is near the viewport (`fx.near`, rootMargin 100 px for the hero and Susun, 0 for Music); `fx.flush` runs it once more after it switches off so it settles. Effects: the hero, the Susun scrub, the Music lyric, and fitCheck (on while the section state is panel or clipboard and no panel or toast rules). No per-frame `querySelectorAll` (lists are built once), no wheel or touch interception, no scroll-snap, no parallax, no smooth-scroll library. The lyric animation restarts by swapping two identical keyframe classes (`swap-a`/`swap-b`) instead of forcing a reflow.

- **Hero:** `q = clamp(-hero.top / hero.height, 0, 1)`; the headline scales `1 - 0.08q` (to 0.92) and fades `1 - 0.8q` (to 0.2), both ways; `will-change` only while the hero is near the viewport.
- **Reveal** (h2, h3 and screenshots only; never the Susun stage): once, as before. CSS hides `.js .reveal:not(.is-in)` (opacity 0, `translateY(16px)`), 500 ms transition; one IntersectionObserver that unobserves on first intersection; what is in view at start shows at once.
- `scroll-behavior: smooth` only under `prefers-reduced-motion: no-preference`.

## The shared markup contract (index, 404, privacy, terms stay in step)

Every `<head>`, in order: charset, viewport, title, description, `link rel=icon href=img/logo.svg`, the inline script (identical on all four pages), `style.css`, then `<script src="site.js" defer>`. The body starts with `a.skip`, then `header.site-header` (brand, `button.menu-btn`, `nav#site-nav.site-nav` with Features, Try the cat, Install, Privacy, Terms, GitHub, `button.theme-btn[hidden]`), `main#main`, `footer.site-footer`. On `index.html` the first three nav hrefs are `#features`, `#cat`, `#install`; elsewhere `index.html#...`; the current page's link has `aria-current="page"`. The header markup and metrics (56 px row, link padding, the 76em menu breakpoint, the 23.5em three-line Menu button) are unchanged from the first rebuild because they are measured against the idle notch; only colours and the pill shape of the buttons changed. The header is still not sticky. Legal pages: `main#main.legal > article.legal-doc` with `h1`, `p.legal-updated`, `div.legal-summary`, `nav.legal-toc`, `section[id]` blocks, `blockquote.legal-quote`, tables in `div.table-wrap[role=region][tabindex=0]` (scrolls inside itself); 68ch column. Privacy ids: summary, google-user-data, network, files, read-locally, deleting-data, children, changes, contact. Terms ids: the-app, licence, google, acceptable-use, at-your-own-risk, no-warranty, liability, trademarks, changes, contact. The legal pages' markup did not change in this redesign (only CSS), and their `<main>` text, ids and table cells were diffed against the previous HEAD: identical. If you change the header or footer, change all four files.

`site.js` runs on every page; each part no-ops when its elements are missing and runs inside `safe()`: theme, menu, copy buttons, cats, the scroll loop, the notch, the hero, Susun, reveal; `ready` is set last. The inline head script adds `html.js`, removes it at `DOMContentLoaded` unless `ready` is set, adds `html.reveal-off` after 4 s if `ready` is still missing, drops `js` at 20 s as a last resort, and reads the stored theme.

## Without script, and reduced motion

Without script (`html` has no `.js`): all content readable; the Susun section has normal height, a static stage and the final arranged drawing; the notch is hidden; Copy buttons and their status spans hidden, the commands selectable in one column; the nav links wrap under the brand and stay inside the viewport at 375 px; Menu and theme buttons hidden; the `no-js-only` statements shown (the panel opens on hover in Vale; the cat follows the pointer, four coats and six accessories); reveal content visible.

Reduced motion (emulated): no hero scale or fade (`transform: none`, opacity 1), no `.is-scrub` (Susun static and final, notch idle through it), `.reveal` visible with no transform or transition, notch and crossfade transitions `0s`, `scroll-behavior: auto`, no EQ or lyric animation, dock and cat without easing.

## The check script

`python -I scripts/check-site.py` (standard library only, exit 1 on any problem): parsing and nesting, unique ids, every internal href, src and CSS `url()` points at an existing file (exact case) and every `#fragment` at an existing id (sprite ids included), tables in `div.table-wrap[role=region][tabindex=0]` with a name, `.reveal` only on h2, h3 and figure (h2 is new: features now have h2 headings), no third-party resource, the same inline head script and `style.css`/`site.js` on every page, header and footer identical apart from the `index.html#` prefix and `aria-current`, no U+2014 under `site/`, and (new) the dock on index.html has the 14 tabs of `Page::ALL` in order, each controlling an existing panel. The new dock check was run against the previous HEAD's `site/` (13 tabs) and reported it.

## Screenshots

From `assets/screenshots` into `site/img`: music, notch, clipboard, clipboard-copied, system, cleaner, screenshot, shelf, cli, search-files, calendar, editor. The screenshots were last regenerated on 2026-10-29, before the Devices and Notifications pages existed, so in the `site/img` copies of the panel shots (calendar, cli, clipboard, music, notch, shelf, system) the old page-dot row is painted over with #17120F; `assets/screenshots` keeps the originals. When `scripts/capture-screenshots.ps1` ([[screenshot-capture-script]]) is re-run, copy the new files over and drop this note. There is no Susun, Devices or Notifications screenshot; nothing is faked (the Susun picture says it is an illustration).

## Verified, and not

Run against `python -m http.server --directory site` on 127.0.0.1 in headless Chrome 156 through the DevTools protocol (a small Node script, one page at a time, cache disabled):

- **Layout:** index, privacy, terms and 404 at 1280 x 800 and 375 x 812, dark and light: `scrollWidth <= clientWidth` on all 16, no console error or exception, every request to the site's own origin. Screenshots looked at: hero, Susun at p = 0, 0.5 and 1 (1280 dark and light, 375 dark, 414 light), Music, System, the compact lists, Try the cat, Install, the open dock on the Arrange page, privacy at 1280 dark, 404 at 375 light.
- **Theme:** no stored choice, system light or dark: no `data-theme`, page rgb(23, 18, 15), `color-scheme: dark`, button "Switch to light theme"; a click sets light and stores it, a second click dark.
- **Menu:** at 375 px on all four pages it opens (`aria-expanded` true) and Escape closes it.
- **States:** Music section music, the compact list pomodoro, `#cat` cat, Install idle; Susun pinned arrange (step 0 "Tile 1 of 3", step 2 "Tile 3 of 3", step 3 "Arranged"). A real click on a Copy button: clipboard state with the command as preview, "Copied" on the button and in the status, idle again 3.4 s later. Hover: a sweep across the notch in 60 ms steps does not open it, an 80 ms rest does not, 280 ms does; leaving closes; a click pins; a click outside closes.
- **Dock:** 14 tabs in order; Enter on the face opens with focus on the selected tab; ArrowRight from CLI lands on Arrange (the only tabindex 0); Enter shows `pg-arrange` and the title "Arrange"; tabs span 430 to 850 inside the open notch (410 to 870); the hovered icon scales to 1.897; Escape returns focus to the face.
- **Reversibility:** down the page: step 0 at the top, 2 at p = 0.61, 3 at the end, hero `scale(0.92)` / 0.2; back up: step 0 at p = 0.09, hero back to no transform and opacity 1 at the top. 46 `transitionrun` events on `.reveal` (23 elements x opacity and transform), all in the first pass, none on the way up or on a second pass down; 23 of 23 keep `is-in`.
- **Reduced motion** (emulated): hero at scroll 400 `transform: none`, opacity 1, `will-change: auto`; Susun not `.is-scrub`, stage static, wrapper 623 px = stage, no `data-step`, notice shown; notch idle through Susun; 23 of 23 `.reveal` visible with no transform and 0 s transition; notch transitions 0 s; `scroll-behavior: auto`.
- **Without script** (script execution disabled) at 1280 and 375: no Copy button or status, no hint, notch, Menu, theme button, picker or bubble; 6 nav links visible and inside the viewport; 3 `no-js-only` statements; Susun wrapper = stage height, static, fills at opacity 1, notice shown; 0 of 23 `.reveal` hidden; commands in one column, `user-select: text`; no overflow. Screenshots at 375 looked at.
- **Failsafe:** `site.js` answered 404: no `js` class at 0.1 s on all four pages, nothing hidden, no Copy buttons. `site.js` delayed 7 s: `js` from the start, `reveal-off` by 4.4 s (the 0.5 s fade then runs; 0 of 23 hidden at 6.8 s), `ready` by 7.6 s, `js` still there at 21 s, Copy buttons shown throughout.
- **fitCheck scan:** the table above, 0 intersections in four viewports; control 86.
- **Contrast sweep:** 0 failures, see the tokens section.
- **Legal text:** `<main>` text, section ids and table cells of privacy.html and terms.html identical to the previous HEAD. `python -I scripts/check-site.py`: 0 problems. `site/img/logo.svg` byte-identical to `assets/logo.svg`. No U+2014 under `site/`. `cargo test --locked`: 1705 passed, 6 ignored, no compiler warnings (Rust unchanged).

Not verified: a real phone (no touch events were generated), Safari and Firefox (`100svh`, `behavior: 'instant'` and sticky were only run in Chrome), a screen reader, scroll smoothness on a real mid-range laptop (the loop was read for per-frame work, not profiled), the compensation branch for a scrub switch while the wrapper is above the view (a load at `index.html#install`, `#cat` or `#google-calendar` at 1280 and 375 ends with the target 56 px from the top, its scroll margin, but in Chrome the scrub switches on in the first frame, before the smooth fragment scroll moves, so that branch did not run).

Related: [[arrange]], [[panel-pages]], [[cat-colours-and-surfaces]], [[cat-accessories]], [[notch-peek-eyes]], [[cat-looks-at-pointer]], [[eq-follow-audio]], [[readme-structure]], [[studio-bata-logo-sources]], [[logo]], [[github-pages]]
