---
tags: [decision, site, pages, design, a11y]
date: 2026-11-01
---

# The landing page: a working notch at the top edge

Rebuilt from scratch in `site/` (static, English, no build step, published as is by [[github-pages]]). The owner chose direction "A": the browser's top edge is the top of a Windows screen, and **one bold element is a real, working notch** fixed at the top centre. Everything else is quiet: large left-aligned DM Sans headlines, whitespace, and sections that change composition (text-left and image-right, the reverse, a stacked pair of images, two short columns, a full-width pair of images, a plain list). Dials: ENERGY 2, RHYTHM 2, MOTION 2. The brand mark is still the terracotta cat head ([[logo]]); the page never invents another logo.

Not used, on purpose: a card grid, bento, a fake terminal window, eyebrow labels, uppercase tracked labels, middle-dot meta strings, arrows on buttons, stats, testimonials, an FAQ, em dashes.

## The notch (`#notch`, driven by `site.js`)

A fixed container with a `button.notch-face` (closed states, `aria-expanded`, `aria-controls="notch-panel"`) and `#notch-panel` (hidden when closed). Closed visuals are `aria-hidden`; scroll-driven changes are not announced. Sections carry `data-notch`; an IntersectionObserver on a thin band (rootMargin `-40% 0 -55% 0`) picks the last section in the band, and keeps the previous state when none is in it.

| State | Section | What it shows |
|---|---|---|
| idle | hero and every section without its own state | the terracotta cat left, the visitor's 24 h clock right (DM Mono, one `setTimeout` aimed at the next minute, only while idle and the tab is visible) |
| music | `.f-music` | a plain square cover, a demo lyric line, 5 EQ bars. Demo track "Demo track" by "Vale demo"; five original lyric lines; the line follows the section's scroll progress (passive scroll listener only while the section intersects, work in rAF). EQ is CSS keyframes, paused when the tab is hidden |
| panel | `.f-panel` | a "peek" (title, one demo line, the dock row), 400 x 128, only while no text lies under it; otherwise the idle notch (see "The peek must not cover text") |
| clipboard | `.f-clip`, and 3 s after any Copy button | "Copied" with a preview of the copied text; while any of the header is on screen (`.is-compact`, an IntersectionObserver on `.site-header`), or when the section state would cover text (`.is-tight`), only the cat and "Copied", at the idle size. The 3 s toast after a Copy button ignores `.is-tight` |
| pomodoro | `.f-pomodoro` | a ring at a fixed progress with a static 18:42, no countdown |
| cat | `#cat` | the cat, following the pointer, in the chosen coat and accessory |

Sizes: idle and cat 190 x 34 (120 x 45 below 32.5em, so the face is a 44 px tap target; 96 x 45 below 23.5em), music 300 x 36 (300 x 45 there), clipboard 340 x 56 (the idle size while the header is visible), pomodoro 200 x 34 (same as idle below 32.5em), open 460 x 300 (viewport minus 32 px on phones). Width, height and radius ease in about 350 ms, content crossfades.

**Interaction.** Mouse hover opens the full panel only after the pointer rests on the notch for 120 ms (the shelf's `OPEN_DELAY_MS`; moving more than 4 px restarts the wait, leaving cancels it), because the idle notch (y 0..34) overlaps the nav row (y 6..50) and a sweep along the header from 1216 to 1280 px would otherwise open the panel over Features and Try the cat. Leaving closes it (160 ms grace) unless it was pinned; a click pins, a second click closes. Touch: pointerenter is ignored, a tap toggles. Keyboard: the face is focusable, Enter or Space opens and moves focus to the selected tab (with `preventScroll`, since the notch is still narrow then; the dock is scrolled to the tab only after the width `transitionend`, never on `transitioncancel`, which a close interrupted by the reopen fires early), Escape closes and returns focus to the face, ArrowLeft/Right/Home/End move between the 13 icons (roving tabindex), Enter, Space or click selects. A click outside closes a pinned panel; Tab out of the panel closes it. The open panel's header strip is the face button itself, so clicking the title still toggles.

**Dock.** 13 `role="tab"` buttons in the app's order (Calendar, Pomodoro, Reminders, Notes, Music, Mixer, Clipboard, Notifications, File shelf, System, Devices, CLI, Shortcuts) and 13 `role="tabpanel"` demo pages. Icons are the `Icon::*` paths of `src/ui/icons.rs` (24 grid, 1.5 round stroke, duotone fill at 0.38) as `<symbol>`s in a hidden sprite. Magnification as in `src/ui/dock.rs`: scale `1 + 0.9 * falloff(d)`, `falloff = 0.5 * (1 + cos(pi d / R))`, `R = 2 x pitch`, eased per frame with tau 70 ms, rAF stops when settled, icons grow from their bottom edge, the glyph has `pointer-events: none` so hit targets never change. On a coarse pointer or below 32.5em: no magnification, 44 x 44 tabs in a row that scrolls inside the panel (`overscroll-behavior: contain`).

**The cat** (`#big-cat`, `#notch-cat`): drawn from `src/ui/robot.rs` (head, ears with a 5 unit round stroke, eye pills 5.6 x 10 at x 24 and 40) and `cat_style.rs` (patches, accessories). Look = head slide 17.23 grid units, tilt 6 degrees around (32,40), extra eye slide 9.85 units, that is the app's 7 / 6 / 4 dip on a 26 dip cat (k = 26 / 64). `v = clamp(offset / distance to that side's viewport edge, -1, 1)`, exponential ease tau 120 ms, rAF only while moving, targets that change by under 0.02 are ignored. Touch: toward the last tap. Reduced motion: no easing, update only on pointerdown. Paint order from [[cat-accessories]]: rim (Black only), silhouette, patches, eyes, accessories. Calico patches are cut with a `<mask>` of head plus ears filled and stroked white (an SVG `clipPath` ignores strokes). Coat and accessory are native radio groups in `fieldset`/`legend`; the choice also restyles the notch's cat (the header logo never changes). The look is read from the checked radios at start and on `pageshow`, because browsers restore radio state on back/forward (Firefox also on reload) without a `change` event. Greeting from the visitor's hour: 04-10 Good morning, 11-17 Good afternoon, 18-23 Good evening, 00-03 Up late? ([[greeting-periods]]).

## The shared markup contract (index, 404, privacy, terms stay in step)

Every `<head>`, in order: charset, viewport, title, description, `link rel=icon href=img/logo.svg`, the inline script (adds `html.js`, reads `vale-theme` from localStorage in try/catch into `html[data-theme]`), `style.css`, then `<script src="site.js" defer>`. The body starts with `a.skip`, then `header.site-header` (brand, `button.menu-btn`, `nav#site-nav.site-nav` with Features, Try the cat, Install, Privacy, Terms, GitHub, `button.theme-btn[hidden]`), `main#main`, `footer.site-footer`. On `index.html` the first three nav hrefs are `#features`, `#cat`, `#install`; on every other page they are `index.html#...`; the current page's link has `aria-current="page"`. Legal pages: `main#main.legal > article.legal-doc` with `h1`, `p.legal-updated`, `div.legal-summary`, `nav.legal-toc`, `section[id]` blocks, `blockquote.legal-quote`, and tables in `div.table-wrap[role=region][tabindex=0]` (it scrolls inside itself). The legal text column is 68ch. Privacy section ids: summary, google-user-data, network, files, read-locally, deleting-data, children, changes, contact. Terms ids: the-app, licence, google, acceptable-use, at-your-own-risk, no-warranty, liability, trademarks, changes, contact. If you change the header or footer, change all four files.

`site.js` runs on every page and each part no-ops when its elements are missing: theme button (fills an inline SVG and an aria-label, flips `data-theme`, stores `vale-theme` in try/catch, still toggles for the visit without storage), menu (below 76em: `aria-expanded` kept in sync, Escape closes and returns focus to the button, a link click closes, focus moves into the menu), copy buttons (`data-copy`, `navigator.clipboard` with a textarea and `execCommand` fallback, "Copied" for 2 s in an `aria-live` region, a visible "Copy failed. Select the text and press Ctrl+C." when both fail), notch, cat, reveal fallback. Every `<head>` also carries the same inline script: it adds `html.js`, starts a 4 s timer that removes `js` again unless `html.ready` is set, and reads the stored theme. `site.js` runs each part inside `safe()` (one failing part never stops the others), adds `js`, and sets `ready` last. So a blocked, missing or broken `site.js` ends in the no-script page after 4 s, and nothing stays hidden. `scripts/check-site.py` checks that the inline script is identical on all four pages.

## Without script (`html` has no `.js`)

Two helpers in `style.css`: `html:not(.js) .js-only { display: none !important }` and `.js .no-js-only { display: none !important }`. They only hide, so an element keeps its own `display`.

- **Hidden without script (`js-only`):** the four Copy buttons and their status spans (the command text stays visible, `user-select: text`, and the `.cmd` grid drops to one column so no gap is left), the hero hint, the notes about what the island above is doing, the sentence in the Features lead that says "Hover the island and the panel opens", the interactive half of the Try the cat lead, and the whole coat and accessory picker column (radios that do nothing).
- **Plain statements instead (`no-js-only`):** "In Vale, the panel opens when you hover the island." and "In Vale the cat follows your pointer. It comes in four coats and six accessories, and it greets you by the time of day." (the app has seven accessory choices counting None.)
- **The greeting bubble** is `hidden` in the HTML and `.cat-bubble[hidden] { display: none }`; `site.js` writes the text and then removes `hidden`, so it is never visible empty and never flashes empty.
- **The notch is not shown** (`html:not(.js) #notch { display: none }`). A static idle picture with a button that does nothing looked like a dead control; hiding it also removes the focusable face. (404.html keeps its own `aria-hidden` static decoration, which is not a control.)
- **Navigation:** the Menu button is `display: none` until `.js` is set, the links wrap under the brand (`html:not(.js) .nav-inner`), checked at 375 px. The theme button stays `hidden` until `site.js` fills it.
- Reveal content is fully visible: the hiding rule exists only under `.js`.

## Decisions that differ from the first idea

- **The header is not sticky.** The brief said sticky; but the notch is the one fixed element and its peek state is 400 px wide, which would sit on a sticky bar's links. A scrolling header never collides with a wider notch. Anchors use `scroll-margin-top: 3.5rem` so headings clear the notch.
- **The menu button takes over below 76em (1216 px).** At 1216 px the links end 15 px from the idle notch; at 1280 px they start 48 px away; at 1024 px and below the menu button is shown. From 376 to 520 px the idle notch is 120 px wide and the text Menu button (57 px) clears it (at 376: notch ends 248, menu from 254.9). Below 23.5em (376 px) that no longer fits (21 px of overlap at 320), so the idle, cat, pomodoro and compact clipboard notch shrink to 96 px (9 px side padding: cat 26 + gap 10 + clock 39 fill the 94 px inside the edges) and the Menu button becomes a 44 px three-line button drawn with borders (forced colours keep it), its text kept at `font-size: 0` as the accessible name. Measured: 320 px notch 112 to 208, menu from 212; 360 px notch 132 to 228, menu from 252. The `.nf-idle` side padding needs `.notch .nf-idle` to beat the later `.nf { padding: 0 20px }`.
- **The peek loses its dock row on phones (below 32.5em)** and is 64 px tall, so it does not cover the section text scrolling beneath it.
- **The shrink-to-eyes pill is not built.** The brief allowed skipping it; it would have fought the hover-to-open flow.
- Sections that do not change the notch say `data-notch="idle"` explicitly, so every section has a state.
- **Reveal** (feature headings and screenshots only, once, for every browser): CSS hides `.js .reveal:not(.is-in)` (opacity 0, `translateY(16px)`) and `.js .reveal` transitions opacity and transform over 500 ms. `site.js` shows at once whatever is already in view or above the fold at start, and gives the rest `.is-in` through one IntersectionObserver that calls `unobserve` on first intersection, so nothing can replay when you scroll back. No IntersectionObserver: everything shows at once. Reduced motion: the CSS keeps `.reveal` at opacity 1, no transform, no transition, and the JS skips the observer. The first version used `animation-timeline: view()` where supported; it was dropped because a scroll-linked animation replays whenever you scroll back up.
- **No scroll hijack:** no wheel or touch interception, no snap, no parallax. `scroll-behavior: smooth` only under `prefers-reduced-motion: no-preference`.

## Tokens and measured contrast (WCAG ratios; same formula as the antislop checker)

Custom properties on `:root`; dark under `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) }` and `:root[data-theme="dark"]`; `body` has an explicit background.

| Token | Light | Dark |
|---|---|---|
| page background | #F6F6F4 | #17120F |
| tint band (Try the cat, legal summary) | #ECEBE7 | #1E1815 |
| text | #17120F (17.18) | #F4EDE4 (16.00) |
| secondary | #5C524A (7.03, 6.38 on the band) | #A89A8B (6.78, 6.40 on the band) |
| terracotta ink (links, focus) | #A8431C (5.57, 5.05 on the band) | #E2683C (5.56, 5.25) |
| code background | #EBEAE6 (text 15.44, ink 5.01) | #221B17 (text 14.61, ink 5.08) |
| control border (3:1) | #7A6E65 (4.57, 4.15 on the band) | #8A7D70 (4.64, 4.39) |
| hairline (decoration only) | #DCD8D3 | #3A302A |

Primary button: #E2683C fill, #17120F text, 5.56 (hover #EC7C55, 6.71). Notch, same in both themes: surface #17120F, text #F4EDE4 (16.00), secondary #A89A8B (6.78), accent #E2683C (5.56), focus ring #F4EDE4 (16.00, drawn inside). In dark theme the notch has a 1 px edge #75675B (3.41 on the page). A DOM sweep of every text node against its effective background found no pair under 4.5:1 (3:1 for large) in either theme.

## Screenshots used, and why the others were not

Copied from `assets/screenshots` into `site/img`: music, notch (the Calendar week page, used for "Thirteen pages in one panel"), clipboard, clipboard-copied, system, cleaner, screenshot, shelf, cli, search-files, calendar, editor (the Google Calendar event editor, not a screenshot editor), pomodoro, reminders, notes, robot, greeting. All are 1x and shown at or below their natural width with `width`/`height`, `loading="lazy"` and specific alt text.

Not used: `music-lyrics.png` (shows the old lyric offset control, -0.3 s with -/+ buttons, gone since 3.9, see [[lyrics-sync]]), `tour.png` (1.3 MB), `notices.png` (203 KB APNG; `greeting.png` says the same in 3 KB), `search.png` (search-files covers more), the settings, cleaner-mole and other detail shots. There is no Devices or Notifications screenshot, so those two sections have none (nothing is faked).

The screenshots were last regenerated on 2026-10-29, before the Devices and Notifications pages existed, so their page-dot row showed 11 dots, not 13. In the `site/img` copies of the ten panel shots with that row (calendar, cli, clipboard, music, notch, notes, pomodoro, reminders, shelf, system) the dot row is painted over with the panel colour #17120F (a box around the dots only, so the rounded corners stay); `assets/screenshots` still has the originals, so those ten files differ from `assets/`. When `scripts/capture-screenshots.ps1` ([[screenshot-capture-script]]) is re-run for 3.9, copy the new files over and drop this note.

## The peek must not cover text

The notch is fixed at top centre. Its `panel` peek (400 x 128, 64 high below 32.5em) and the section's `clipboard` state (340 x 56) are wider than the idle notch and could lie over the end of a text column (the Music text at 1280 px, for one). Goal: while the notch is settled in either state, its box meets no text element (h1, h2, h3, p, li, figcaption, code, pre, label, legend, td, th, blockquote) at any scroll position. The small states (idle, music, pomodoro, cat, at most 45 px high) may pass over text, like a real notch, and so may the 3 s copy toast.

**Chosen approach: the state asks, the page answers (`fitCheck` in `site.js`).** The band under the notch is y 0 to peek height + 16 px, x peek width + 8 px each side, around the centre. While the section's state is `panel` or `clipboard`, on scroll frames (one rAF), resize and load only, the script reads the rects of the text elements (the list is built once and leaves out the notch's own panel text). A text element in the band: `panel` shows the idle notch, `clipboard` keeps its content at the idle footprint (`.is-tight`). Shrinking is immediate; growing waits 150 ms of free band, so the notch does not flap while text streams past. Entering a section decides at once. An open panel (hover or click) ignores all of this and may cover the page. Why not reserve layout: at 1280 px the text column runs from x 64 to 608 and the screenshot column starts at 656, so keeping both out of a 416 px band around x 640 would mean narrowing the columns to about 370 px, and at 768 px there is nothing to reserve. The cost of the chosen approach: at 1024 px and wider the panel peek hardly ever shows (the gap between two sections' text is about 160 px, the band is 144 px plus 16), and the clipboard state shows in full only in the gap after the clipboard section. At 768 px, where sections are one column, it shows more often. The section demo is a nicety; the full panel on hover or click is unchanged.

Scan result (2026-11-01, headless Chrome through the DevTools protocol, one page at a time, light scheme; the page-side snippet scrolls in 40 px steps, waits 200 ms, waits until the notch's animations end and its box is the same 120 ms apart, then compares the box with every text element when the state is `panel` or `clipboard` and the notch is more than 45 px high):

| Viewport | Steps | Settled at full panel or clipboard size | Intersections |
|---|---|---|---|
| 1280 x 800 | 244 | 2 | 0 |
| 1280 x 1024 | 239 | 2 | 0 |
| 1024 x 800 | 234 | 2 | 0 |
| 1024 x 1024 | 229 | 2 | 0 |
| 768 x 800 | 326 | 17 (panel peek in 6 of them) | 0 |
| 768 x 1024 | 320 | 15 (panel peek in 5 of them) | 0 |

Control: with the band test forced to "free", the same scan at 1280 x 800 reports 59 intersections (the Music text under the 400 x 128 peek, first at scroll 1120), so the scan does see the problem. Run the scan one page at a time: several pages in one browser starve each other of frames and the section states never change.

## The check script

`python -I scripts/check-site.py` (standard library only, exit 1 on any problem, prints each problem and a final count). For every `site/*.html`: it parses, tags nest and close, ids are unique, every internal href, src and CSS `url()` points at an existing file and every `#fragment` at an existing id (including `index.html#features`, `#cat`, `#install`, the legal TOC and the sprite ids), every table is inside `div.table-wrap[role=region][tabindex=0]` with a name, `.reveal` sits only on headings and figures, no third-party resource is loaded, all four pages load `style.css` and `site.js` with the same inline head script, and header and footer match index.html apart from the `index.html#` prefix and `aria-current`. No text file under `site/` may contain U+2014. External links are listed, not fetched.

## Reduced motion

`prefers-reduced-motion: reduce` turns off smooth scrolling, the reveal (CSS keeps `.reveal` visible with no transform or transition, and the JS does not observe), the EQ animation (static bars at fixed heights), the lyric swap animation, and the dock's easing (scales apply at once); the cat does not ease and moves only on pointerdown; the notch's width, height, radius and crossfades switch at once (`transition: none`). The panel is hidden again without the 380 ms wait. Run with the setting emulated for the reveal (see "Verified, and not"); the rest was read in the CSS and JS.

## Verified, and not

Run against `python -m http.server --directory site` on 127.0.0.1, in headless Chrome through the DevTools protocol (and earlier in the Browser pane, which renders at about 2 frames a second in the background and so is too slow for scroll scans):

- **Layout:** index, privacy, terms and 404 at 1280 x 800 and 375 x 812, light and dark: `scrollWidth <= clientWidth` on all 16, no console error or exception, every request answered 200 or 304 and only to the site's own origin (site.js, fonts, images included). The privacy tables are 776 px in a 776 px box at 1280 and 544 px in a 341 px box at 375 (they scroll inside `.table-wrap`).
- **Reveal once:** 32 `.reveal` elements, none shown at load; scrolling down, up and down again gave 64 `transitionrun` events (32 elements x opacity and transform), all during the first pass, none on the way back, all 32 keep `is-in`.
- **Reduced motion** (emulated): 32 of 32 `.reveal` at opacity 1, `transform: none`, transition duration 0.
- **Without script** (script execution disabled) at 1280 and 375: no Copy button or status span, no hint, no greeting bubble, no picker, no notch, the two `no-js-only` statements shown, six nav links visible and inside the viewport, the Menu and theme buttons hidden, the commands selectable (`user-select: text`) in one column, nothing hidden by reveal, no horizontal overflow. Looked at in screenshots at 375 px.
- **Menu and theme** at 375 px on all four pages: menu opens (`aria-expanded` true) and Escape closes it, theme toggles `data-theme` and back, no console errors.
- **Failsafe:** with `site.js` blocked, at 0.5 s `html` is `js`, 32 `.reveal` hidden; at 4.7 s `html` has no class, nothing hidden, Copy buttons and notch gone. (Not run: a slow connection where `site.js` arrives after 4 s and puts `js` back.)
- **Peek fit:** the scan above, 0 intersections in six viewports.
- `python -I scripts/check-site.py`: 0 problems. `cargo test --locked`: 1618 passed (Rust is unchanged).

Not verified: a real phone (touch events were not generated), Safari and Firefox, a screen reader, the dark-scheme scan (only light was scanned; the check is geometry, not colour).

Related: [[panel-pages]], [[cat-colours-and-surfaces]], [[cat-accessories]], [[notch-peek-eyes]], [[cat-looks-at-pointer]], [[eq-follow-audio]], [[readme-structure]], [[studio-bata-logo-sources]]
