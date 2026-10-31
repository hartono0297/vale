---
tags: [decision, site, pages, design, a11y]
date: 2026-10-31
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
| panel | `.f-panel` | a "peek" (title, one demo line, the dock row), 400 x 128 |
| clipboard | `.f-clip`, and 3 s after any Copy button | "Copied" with a preview of the copied text |
| pomodoro | `.f-pomodoro` | a ring at a fixed progress with a static 18:42, no countdown |
| cat | `#cat` | the cat, following the pointer, in the chosen coat and accessory |

Sizes: idle 190 x 34 (120 x 45 below 32.5em, so the face is a 44 px tap target), music 300 x 36 (300 x 45 there), clipboard 340 x 56, pomodoro 200 x 34, open 460 x 300 (viewport minus 32 px on phones). Width, height and radius ease in about 350 ms, content crossfades.

**Interaction.** Mouse hover opens the full panel and leaving closes it (160 ms grace) unless it was pinned; a click pins, a second click closes. Touch: pointerenter is ignored, a tap toggles. Keyboard: the face is focusable, Enter or Space opens and moves focus to the selected tab, Escape closes and returns focus to the face, ArrowLeft/Right/Home/End move between the 13 icons (roving tabindex), Enter, Space or click selects. A click outside closes a pinned panel; Tab out of the panel closes it. The open panel's header strip is the face button itself, so clicking the title still toggles.

**Dock.** 13 `role="tab"` buttons in the app's order (Calendar, Pomodoro, Reminders, Notes, Music, Mixer, Clipboard, Notifications, File shelf, System, Devices, CLI, Shortcuts) and 13 `role="tabpanel"` demo pages. Icons are the `Icon::*` paths of `src/ui/icons.rs` (24 grid, 1.5 round stroke, duotone fill at 0.38) as `<symbol>`s in a hidden sprite. Magnification as in `src/ui/dock.rs`: scale `1 + 0.9 * falloff(d)`, `falloff = 0.5 * (1 + cos(pi d / R))`, `R = 2 x pitch`, eased per frame with tau 70 ms, rAF stops when settled, icons grow from their bottom edge, the glyph has `pointer-events: none` so hit targets never change. On a coarse pointer or below 32.5em: no magnification, 44 x 44 tabs in a row that scrolls inside the panel (`overscroll-behavior: contain`).

**The cat** (`#big-cat`, `#notch-cat`): drawn from `src/ui/robot.rs` (head, ears with a 5 unit round stroke, eye pills 5.6 x 10 at x 24 and 40) and `cat_style.rs` (patches, accessories). Look = head slide 17.23 grid units, tilt 6 degrees around (32,40), extra eye slide 9.85 units, that is the app's 7 / 6 / 4 dip on a 26 dip cat (k = 26 / 64). `v = clamp(offset / distance to that side's viewport edge, -1, 1)`, exponential ease tau 120 ms, rAF only while moving, targets that change by under 0.02 are ignored. Touch: toward the last tap. Reduced motion: no easing, update only on pointerdown. Paint order from [[cat-accessories]]: rim (Black only), silhouette, patches, eyes, accessories. Calico patches are cut with a `<mask>` of head plus ears filled and stroked white (an SVG `clipPath` ignores strokes). Coat and accessory are native radio groups in `fieldset`/`legend`; the choice also restyles the notch's cat (the header logo never changes). Greeting from the visitor's hour: 04-10 Good morning, 11-17 Good afternoon, 18-23 Good evening, 00-03 Up late? ([[greeting-periods]]).

## The shared markup contract (index, 404, privacy, terms stay in step)

Every `<head>`, in order: charset, viewport, title, description, `link rel=icon href=img/logo.svg`, the inline script (adds `html.js`, reads `vale-theme` from localStorage in try/catch into `html[data-theme]`), `style.css`, then `<script src="site.js" defer>`. The body starts with `a.skip`, then `header.site-header` (brand, `button.menu-btn`, `nav#site-nav.site-nav` with Features, Try the cat, Install, Privacy, Terms, GitHub, `button.theme-btn[hidden]`), `main#main`, `footer.site-footer`. On `index.html` the first three nav hrefs are `#features`, `#cat`, `#install`; on every other page they are `index.html#...`; the current page's link has `aria-current="page"`. Legal pages: `main#main.legal > article.legal-doc` with `h1`, `p.legal-updated`, `div.legal-summary`, `nav.legal-toc`, `section[id]` blocks, `blockquote.legal-quote`, and tables in `div.table-wrap[role=region][tabindex=0]` (it scrolls inside itself). The legal text column is 68ch. Privacy section ids: summary, google-user-data, network, files, read-locally, deleting-data, children, changes, contact. Terms ids: the-app, licence, google, acceptable-use, at-your-own-risk, no-warranty, liability, trademarks, changes, contact. If you change the header or footer, change all four files.

`site.js` runs on every page and each part no-ops when its elements are missing: theme button (fills an inline SVG and an aria-label, flips `data-theme`, stores `vale-theme` in try/catch, still toggles for the visit without storage), menu (below 76em: `aria-expanded` kept in sync, Escape closes and returns focus to the button, a link click closes, focus moves into the menu), copy buttons (`data-copy`, `navigator.clipboard` with a textarea and `execCommand` fallback, "Copied" for 2 s in an `aria-live` region, a visible "Copy failed. Select the text and press Ctrl+C." when both fail), notch, cat, reveal fallback. Without script: no `html.js`, so the menu button is hidden, the links wrap under the brand, the notch is a static idle picture (its button ignores the pointer and has `tabindex=-1`), the cat is the terracotta one, the pickers render, the hint and the greeting do not.

## Decisions that differ from the first idea

- **The header is not sticky.** The brief said sticky; but the notch is the one fixed element and its peek state is 400 px wide, which would sit on a sticky bar's links. A scrolling header never collides with a wider notch. Anchors use `scroll-margin-top: 3.5rem` so headings clear the notch.
- **The menu button takes over below 76em (1216 px).** At 1216 px the links end 15 px from the idle notch; at 1280 px they start 48 px away; at 1024 px and below the menu button is shown. On a 375 px phone the idle notch is 120 px wide so the brand, the notch, the Menu button and the theme button fit one row with a 16 px gutter (notch 127.5 to 247.5, menu from 254).
- **The peek loses its dock row on phones (below 32.5em)** and is 64 px tall, so it does not cover the section text scrolling beneath it.
- **The shrink-to-eyes pill is not built.** The brief allowed skipping it; it would have fought the hover-to-open flow.
- Sections that do not change the notch say `data-notch="idle"` explicitly, so every section has a state.
- **Reveal** (feature headings and screenshots only, once): `animation-timeline: view()` with `animation-range: entry 0% entry 35%` inside `@supports`; otherwise an IntersectionObserver adds `.is-in` once and unobserves. Content is visible without script and without support (the hide rule exists only under `html.js` and `@supports not`).
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

Copied from `assets/screenshots` into `site/img`: music, notch (the Calendar week page with the dot row, used for "Panel pages"), clipboard, clipboard-copied, system, cleaner, screenshot, shelf, cli, search-files, calendar, editor (the Google Calendar event editor, not a screenshot editor), pomodoro, reminders, notes, robot, greeting. All are 1x and shown at or below their natural width with `width`/`height`, `loading="lazy"` and specific alt text.

Not used: `music-lyrics.png` (shows the old lyric offset control, -0.3 s with -/+ buttons, gone since 3.9, see [[lyrics-sync]]), `tour.png` (1.3 MB), `notices.png` (203 KB APNG; `greeting.png` says the same in 3 KB), `search.png` (search-files covers more), the settings, cleaner-mole and other detail shots. There is no Devices or Notifications screenshot, so those two sections have none (nothing is faked).

Caveat: the screenshots were last regenerated on 2026-10-29, before the Devices and Notifications pages existed, so their page-dot row shows 11 dots, not 13. The pages themselves are unchanged. Re-run `scripts/capture-screenshots.ps1` ([[screenshot-capture-script]]) and copy the files again when convenient.

## Reduced motion

`prefers-reduced-motion: reduce` turns off smooth scrolling, the reveal, the EQ animation (static bars at fixed heights), the lyric swap animation, and the dock's easing (scales apply at once); the cat does not ease and moves only on pointerdown; the notch's width, height, radius and crossfades switch at once (`transition: none`). The panel is hidden again without the 380 ms wait. This was read in the CSS and JS, not run with the setting on (the Browser pane has no emulation for it).

## Verified, and not

Run in the Browser pane against `python -m http.server`: 1280, 1216, 1024, 768 and 375 widths (no horizontal overflow, nav clear of the notch, panel open at 375 does not overflow), light and dark, keyboard open/select/Escape, hover open and the dock magnification, state changes while scrolling, the Copy buttons (success path with a stubbed clipboard, failure message in the pane), theme toggle with storage blocked, menu open/Escape/link, the cat following the pointer in Calico plus Crown, no console errors, no third-party requests. A throwaway legal page with a 5-column table at 375 px scrolled inside `.table-wrap` (544 px in a 341 px box) with no page overflow. Not verified: a real phone (touch events were not generated), Safari and Firefox, a screen reader, `prefers-reduced-motion`, the no-script page (checked by removing the `js` class only). The Browser pane renders slowly in the background, so IntersectionObserver-driven state changes needed about a second there.

Related: [[panel-pages]], [[cat-colours-and-surfaces]], [[cat-accessories]], [[notch-peek-eyes]], [[cat-looks-at-pointer]], [[eq-follow-audio]], [[readme-structure]], [[studio-bata-logo-sources]]
