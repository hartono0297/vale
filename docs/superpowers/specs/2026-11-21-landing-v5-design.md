# Landing page for Vale 5.6: Night mix with today's features

Date: 2026-11-21. Mockup: the "Arah A" and "Arah A di HP" artboards of the
canvas *Vale landing page update* (claude.ai artifact, private to the owner).

## Why

The site in `site/` was built on 2026-11-03 for Vale 3.9 ([[site-landing]]).
Since then Vale got a 15th page (Record: meetings, screen recording,
transcripts, live captions), two more mascots (Falk the cockatiel, an
octopus) with short stories, notch colour modes (Plain, Gradient, Face),
document signing from the file shelf, screenshot rotation, a phone and laptop
link over Wi-Fi or Tailscale, clipboard drag-out, settings export and import,
and the What's new notice. The site still says "fourteen pages", shows only
the cat and never mentions any of the rest. The owner wants the site to show
what Vale does today.

## Goals

1. The hero lets a visitor pick the mascot (Cat, Cockatiel, Octopus) and the
   notch colour (Plain, Gradient, Face); the live notch at the top of the
   page follows both.
2. The story says "One hover, fifteen pages." and includes Record, in
   `Page::ALL` order (Record third).
3. The playground shows the new features next to the best of the old ones,
   eight cards in all.
4. Install and privacy copy match today's app (update flow, export and
   import, recordings, transcripts, captions, signatures, Tailscale).

## Non-goals

- No new section order, no rewrite: direction A keeps the Night mix frame
  (sky, stars, header pills, marquee, pinned story, playground, tint panel,
  footer, go-to-top tail).
- No accessories on the site, no light-theme redesign, no new legal pages.
- The wish constellation, the walking cat and the go-to-top tail stay
  cat-only (see Mascots).
- No release, no version bump.

## Anti-goals (a failure even if it works)

- Any invented number, testimonial, download count or claim. Every sentence
  comes from `README.md`, `docs/guide.md` or `src/whats_new.rs`.
- Losing the current craft: contrast below WCAG AA, a dead control, a broken
  keyboard path, horizontal scroll at 320 px, the pinned story no longer
  fitting, or the hero cat's easter eggs breaking for the cat.
- An em dash anywhere under `site/`.

## Constraints

- Static site, no build step, no framework, no third-party request; local
  Geist / Geist Mono only ([[github-pages]]).
- `scripts/check-site.py` must pass. It hard-codes 14 dock tabs, 14 story
  pages and six playground cards; those expectations change with this work.
- Both themes (dark default, light) and reduced motion must keep working.
- The shared header/footer contract across index, 404, privacy and terms stays.
- Contrast is re-measured with the sweep method in [[site-landing]].

## Approaches (decided in the conversation)

- **A, Night mix with new content** (picked): keep the frame, extend the hero,
  story and playground. Smallest risk to the existing craft.
- B, chapters by task (Closed / Open / After the meeting / Sign / Phone /
  Mascots): a near rewrite of `index.html`, would drop or move the pinned
  story and the playground. Not picked.

Mascot scope (decided, "Opsi 2"):
- Opsi 1, mascots only in the notch and one card: smallest, hero stays cat.
- **Opsi 2** (picked): the hero mascot can be switched; eyes that follow the
  pointer, blinking, the poke jump, rubbing and dragging onto the notch work
  for all three. The cat-shaped extras stay cat-only.
- Opsi 3, every easter egg for all three: most work, mostly unseen.

## The design

### Hero

- Two groups of buttons above the headline, as in the mockup:
  *Mascot* (Cat, Cockatiel, Octopus) and *Notch colour* (Plain, Gradient,
  Face). Real `button`s with `aria-pressed`, 44 px tall, selected state is a
  filled light chip (lightness, not hue alone). They are `.js-only` like the
  coat chips today.
- The coat chips stay and follow the chosen mascot: Cat has Terracotta,
  Calico, White, Black (today's site colours); Cockatiel has Lutino, Grey,
  White face; Octopus has Coral, Red, Purple, Sea blue. Colours, from
  `docs/design/maskot-pilihan-akhir.html` (tracked) and [[maskot-falk-gurita]]:
  - Lutino: body #F1E4A2, face #F6D04A, crest #F6D04A, cheeks #EC6B3A
  - Grey: body #9A959C, face #F2D35A, crest #E9CF62, cheeks #EC6B3A
  - White face: body #9A959C, face #F4EFE6, crest #E4DED6, no cheeks
  - Beak #D9978F; octopus Coral #E87C8E, Red #D9505F, Purple #A98BD6,
    Sea blue #5FA8C9, spots the body mixed 18 % toward black.
  - Shapes: the 64-grid paths in that file (head `FALK`, `CREST` feathers,
    octopus `MANTLE` and arms), scaled to the site's 256 grid by 4.
- Layout, top to bottom (as on the mockup's desktop and 390 px artboards):
  the mascot, the Mascot group, the Notch colour group (the two groups sit on
  one row from about 40em, stacked and centred below), the coat chips, the
  headline, the line, Download, the install box. At 390 px every button keeps
  44 px and the groups wrap; nothing scrolls sideways.
- The hero mascot is inline SVG like the cat today. The cockatiel and octopus
  get the same parts the script drives: eyes (normal, happy, squint),
  blink, and a body group for the jump and tilt. Ears perk and twitch are
  cat-only; the cockatiel's crest and the octopus's arms take that role where
  cheap (crest tilt on hover; arms stay still), else nothing.
- Behaviour per mascot: eye follow, head tilt, blink, greeting bubble, poke
  jump with the bubble cycle, rubbing with hearts, drag and drop on the notch
  with the greet state. The wish (five quick presses) and its head
  constellation run only for the cat; with another mascot five presses are
  five pokes.
- Choice is stored like the theme (`vale-mascot`, `vale-notch-colour`, coat
  per mascot) in try/catch; no storage means Cat, Plain, Terracotta.
- Headline stays "Your Windows, / with a notch." The line under it becomes
  "Music and lyrics, your clipboard, meeting recordings and a small mascot,
  at the top of the screen. Free and open source, one 6 MB exe for
  Windows 11." (both facts are in the README).

### The live notch

- Idle, clipboard and greet states draw the chosen mascot (idle, happy) in
  its coat; the sprite gains cockatiel and octopus symbols next to
  `cat-idle`, `cat-happy`, `cat-sleep`.
- Notch colour follows the app ([[notch-colour-style]], `src/ui/notch_colour.rs`):
  - Plain is today's surface.
  - Gradient is vertical: the surface down to 28 % of the height, then blended
    toward the coat's tint at the bottom, by at most 0.45 and only as far as
    the cream text keeps 5:1 (compute the stop per coat and write it down).
    Notices and the greeting on the site use it too.
  - Face (idle only, like the app's closed compact views): the notch is filled
    with the mascot's face colour and shows the face marks (eyes, the
    cockatiel's cheeks and beak, the calico patches, the octopus's spots) and
    the clock, centred as one group, in ink #17120F where it holds 4.5:1 on
    the fill, else cream. The clipboard and greet states draw as Gradient,
    as in the app. Mascot stories in the playground card still play (the site
    is a demo, not the app's gate).
  - The open panel is unchanged in every mode.
- Coat tints (from `notch_colour::tint`): Terracotta and Calico #E2683C,
  White #F6F1EA, Black #8E7CB8, Lutino #F6D04A, Grey and White face #9A959C,
  octopus its body colour.

### Story

- Heading "One hover, fifteen pages."; counter "NN / 15".
- Record is the third `li[data-page="record"]`, with a demo page and a dock
  icon (a new sprite symbol). Line: "A meeting to .m4a, your screen to .mp4,
  and the transcript written on your PC, saying who spoke."
- The live notch's panel gets the same 15th tab and demo page.
- Pinned fit must still hold at the sizes listed in [[site-landing]].

### Playground (eight cards)

Today's six are dance, typing, peek, susun, draw and copy. Order on the page
(builder's call, settled here): Sign (spans the whole grid row), Draw, Captions,
Phone, Mascots, Typing, Susun, Copy. Grid as today (1, 2, 3 columns), Sign
always the full row with picture and text side by side from 40em.
The Record demo page and the "Signed" notch notice take their content from
`README.md` and `docs/guide.md` (Record: the record button, Mic, PC audio and
Live captions switches, recordings grouped by day with Transcribe / View;
the notice: "Signed" and the new file's name). The Record dock icon is a new
sprite symbol drawn in the style of the other fourteen. Sample caption lines
and the QR picture are builder-made demo content, decorative, never presented
as real data.

| Card | Status | What it does |
|---|---|---|
| Sign a PDF from the shelf | new, full row | A sample page; *Sign the sample page* places a signature, the date and a check (no upload; nothing leaves the page). The notch shows "Signed". |
| Draw right on the screen | kept, extended | Today's draw card plus a *Turn* button (and Ctrl+R while the card has focus) that turns the selection a quarter turn with the strokes. |
| Captions for a call that has none | new | *Play a sample* runs a short scripted caption card: grey line turning white, "Me" and "Others". Text is invented sample dialogue, clearly a demo. |
| Files from your phone, no cable | new | A QR picture and the *Wi-Fi / Tailscale* switch (a real toggle that changes the caption under it). The QR is decorative, not scannable. |
| Three mascots, ten short stories | replaces dance and peek | Picks follow the hero choice; *Play a story* runs one short story around the mini notch per mascot; hovering the picture shrinks the mini notch to the mascot's eyes (today's peek). The dance moves into the story set as the music nod. |
| It types while Claude works | kept | Today's typing card, with the chosen mascot. |
| Susun puts windows in place | kept | Unchanged. |
| Every copy, a preview | kept, extended | Copy as today; the line also says you can drag an image out of the history into a chat. |

### Install and privacy

- Install keeps its command boxes and `<details>`. One paragraph added:
  Vale checks for an update once a day and asks before updating; after an
  update the notch says what changed; Settings > About exports and imports
  settings for another PC.
- Privacy in short: "Stays on your PC" gains recordings, transcripts, live
  captions (never written to disk), signed documents and saved signatures
  (encrypted with DPAPI); "Leaves your PC" gains the Tailscale choice for the
  phone link. `privacy.html` gets the matching rows (recordings folder,
  the transcript engine and model, downloaded only when the user clicks
  Download in Settings, signatures, the phone link in both modes), all worded
  from the README's Privacy section.

### Marquee

Fifteen names, Record included.

## Done means

- `python -I scripts/check-site.py` passes with the new expectations
  (15 dock tabs, 15 story pages, eight cards).
- Contrast sweep: 0 failures at 1280 x 800 and 375 x 812, dark and light,
  with and without reduced motion, for index; privacy re-swept.
- No horizontal scroll from 280 px up; the pill clearance still at least
  12 px; the story still pins at the listed sizes.
- Every new control clicked through with keyboard and pointer, the result
  written down (R-35).
- Merged into `main`, pushed (Pages deploys on push), memory and README
  updated, worktrees cleaned.

## How it is verified

There is no committed sweep script. The contrast sweep, the pin fit and the
pill clearance are re-run the way [[site-landing]] describes (headless
Chrome / the in-app browser, a throwaway script in the session scratchpad,
not committed); `scripts/check-site.py` is the committed gate and gets the
new expectations first (it must fail before the site changes and pass after).

## Left to the builder

- The exact SVG for cockatiel and octopus eye states and their sleep pose.
- Story choreography per mascot in the playground card (one each is enough).
- How the Gradient tint is built in CSS (custom property per mascot).
- Light-theme values for any new token, measured.
