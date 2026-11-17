# Warna notch: Gradasi dan Jadi muka

Date: 2026-11-17. Owner request: "gw mau bikin notchnya juga ada warna sendiri",
then "mau yang sesuai warna maskot juga", then "bikin 2 2 nya, jadi pilihan di
setting", with head hats in Jadi muka "miring di pojok". After the builder check:
"default polos aja, gradasi dan jadi muka jadikan pilihan".

Mockups (open in a browser, all interactive):

- `docs/design/warna-notch.html`: five general directions (A to E) with live contrast.
- `docs/design/warna-notch-maskot.html`: every mascot coat (11) in four colourings.
- `docs/design/notch-muka.html`: Gradasi vs Jadi muka per expression and accessory,
  the hats (the "pinggiran" row is the chosen one), and the shrink to the eyes.

## Intent

The notch is always a dark cutout today. "Warna dasar" (seven near-black surfaces)
changes it so little that it does not read as a colour. The owner wants the notch
to carry the chosen mascot's colour, as identity and decoration (not as a status
signal), in two styles the user can pick in Settings. Nobody's notch changes on
update: the default stays today's plain dark notch.

## Goals

1. A new setting **Warna notch** with three choices: **Polos** (default, exactly
   today's notch), **Gradasi** and **Jadi muka**.
2. **Gradasi:** the island fill is a vertical gradient. The top stays the dark
   surface so the notch still merges with a laptop's black bezel; the bottom takes
   the mascot's colour. The mascot head is drawn as today.
3. **Jadi muka:** the closed notch *is* the mascot's face: filled with the face
   colour, with the face marks (eyes in every expression, cheeks, beak, patches,
   spots, rim) drawn large on the notch instead of the head. Text and icons on it
   turn dark where the face is light. Accessories are relocated (table below).
4. Text and marks stay readable in both: text at least 4.5:1 against the worst
   point of the fill under it, non-text marks at least 3:1.
5. The shrink to the eyes pill ("Mengecil jadi mata") works in both; in Jadi muka
   the colour is already the face, so only the shape shrinks.

## Non-goals

- No colour per state or per content (cover art, recording red). Direction E in
  the first mockup was not chosen.
- No change to the open panel's layout or palette steps (it gets a weaker
  gradient only, below).
- No new Warna dasar swatches, no change to accents.
- No release. Deploy stays with the owner.

## Anti-goals (a failure even if it works)

- Text or an icon on the notch that drops under 4.5:1 / 3:1 in any coat.
- A notch top that no longer merges with a black bezel in Gradasi.
- A mascot that melts into a fill of its own colour with no edge or halo.
- New idle cost: no new timer, thread or continuous clock
  ([[bata-idle-power-budget]]).
- Drawing outside the notch shape (more of the tabs under it covered).

## Constraints

- Rust 2024, Windows 11, Direct2D. One fill site: `Renderer::paint_island`
  (`src/ui/render.rs`), one solid brush today; a vertical gradient template exists
  in `src/ui/card_surface.rs`.
- Every compact and notice painter reads `self.palette`; the dirty-rect signature
  (`Drawn`, `EqKey` in `src/ui/dirty.rs`) must include anything that changes
  pixels.
- Config is lenient (`named_enum!`), island-owned fields go through `merge_owned`,
  and `config_transfer.rs` requires every key classified.
- Code map from the read-only recon: shared notes
  `warna-notch-recon-{1-fill,2-settings,3-mascot,4-ink}.md` (session notes folder),
  summarised where needed below.

## Approaches considered

From the first mockup (A to E):

- **A. Gelap berwarna** (one flat dark tint): simple, but loses the bezel merge.
- **B. Warna penuh** (accent fill, dark ink everywhere, panel too): loudest, but the
  terracotta cat melts (1.00:1), every painter flips ink, and the notch stops
  reading as part of the screen.
- **C. Hitam, tepi berwarna** (1.5 dip coloured rim): cheapest, but reads as black
  from a distance.
- **D. Gradasi dari bezel**: chosen as **Gradasi**, with the mascot's colour.
- **E. Ikut isi** (colour from cover, recording...): not chosen.
- **Jadi muka** came from the earlier "mata maskot" choice (the shrunk pill takes
  the face colour) grown to the whole notch; chosen as the second option.

Hats in Jadi muka: **"pinggiran"** (brim along the top edge, the rest past the
screen edge) vs "miring di pojok" (a small whole hat tilted left of the eyes).
The owner first picked miring di pojok, then switched to **pinggiran** on
2026-11-17: "yang topi di jadi muka, ganti deh mau pake cara jadi muka: pinggiran".

## The design

### Setting

- Settings → Tampilan → card **Island**, a new row **Warna notch** right after
  **Warna dasar**: a segmented control **Polos | Gradasi | Jadi muka** (EN "Notch
  colour": "Plain | Gradient | Face"). Description under it: the chosen name plus
  where the colour comes from, e.g. "Gradasi · dari Terracotta"; Polos says "Gelap
  seperti bezel".
- Config `notch_style: NotchStyle { Plain, Gradient, Face }`, default `Plain`,
  lenient (a typo or a missing key reads as Plain, so old files are unchanged),
  island-owned (`merge_owned`), exported by "Pindah ke PC lain".
- **Polos** draws exactly what the app draws today (same fill call, same
  palette); existing tests that pin today's pixels stay as they are.
- With **Maskot di notch** off (call made after the owner left it to us): the
  **Jadi muka** segment is faded and takes no clicks, with the note "Nyalakan
  maskot dulu" (the "Mengecil jadi mata" pattern). A stored Face is drawn as
  Gradasi (accent tint) while the mascot is off and becomes the face again when it
  is switched back on; the stored value is not rewritten.
- The two Settings previews (Tampilan's island preview and the Maskot preview)
  draw the chosen style.
- Demo variable `DI_NOTCH_STYLE=gradient|face` for every `--demo` scene and
  `--settings`.

### The tint (shared by both styles)

Each coat has one notch colour ("Warna dari"). Calls made in the mockups (the
owner can still correct them):

| Mascot / coat | Gradasi tint | Jadi muka face fill |
|---|---|---|
| Kucing Terracotta | #E2683C (coat) | #E2683C |
| Kucing Calico | #E2683C (orange patch) | #F4EDE4 cream + patches |
| Kucing Putih | #F6F1EA (coat) | #F6F1EA |
| Kucing Hitam | #8E7CB8 (rim) | #1E1B22, amber eyes, rim edge |
| Falk Lutino | #F6D04A (face) | #F6D04A |
| Falk Abu-abu | #9A959C (body) | #F2D35A (face) |
| Falk Muka putih | #9A959C (body) | #F4EFE6 (face) |
| Gurita (4 coats) | body colour | body colour |

Jadi muka's fill is exactly `peek_face(look, surface).fill` (what the shrunk pill
uses today), so the notch and the pill always agree.

### Gradasi

- Fill: vertical linear gradient over the shape's height. Stops: 0 and 0.28 at
  the resolved **Warna dasar** surface (so that setting keeps its meaning and the
  top stays near-black), 1.0 at the bottom colour.
- Bottom colour = the surface blended toward the tint by the largest `t` up to
  **0.45** (closed views and notices) or **0.32** (open panel) such that cream
  text #F4EDE4 holds **5:1** and the caption colour holds **4.5:1** against it.
  Pale coats therefore end up calmer automatically.
- Applies to every view the island draws: closed compact views, notices, the open
  panel. The panel keeps its current palette steps (raised, track...) on top.
- See-through (Isi notch: Tembus) still works: the stops carry `fill_alpha`.
- Marks on the closed compact views: any mark drawn in the accent (equalizer,
  progress, pomodoro focus ring) whose contrast to the bottom colour is under 3:1
  is drawn in the text colour instead.
- Notices and the open panel (call made after the builder check): their tint `t`
  is additionally capped so the accent holds **3:1** against their bottom colour.
  That keeps every accent use there (chips, toggles, sliders, badges, page dots)
  readable without auditing the panel painters. The recording dot gets a 1.5 dip ring in the text
  colour when it is under 3:1.
- The mascot head: when the coat colour is under 3:1 against the bottom colour,
  the existing dark halo (today used by Tembus) is drawn behind the head. The
  Black cat keeps its rim instead.
- With **Maskot di notch** off, the tint is the accent colour.

### Jadi muka

Applies to the **closed compact views** only: Idle, MediaCompact, MediaLyric,
PomodoroCompact, the recording / caption compact views and the screen-recording
countdown (`ScreenCountdown`, so a recording does not flip styles as it starts).
Notices, the greeting and the open panel use **Gradasi** (long text stays on a
dark fill).

- Morph between a Jadi muka view and a Gradasi view (a notice arrives, the panel
  opens): a cross-fade on the existing fill morph. The gradient stops blend from
  the face colour to the gradient, the face marks fade out and the head or the
  panel content fades in; ink switches from dark to cream at the point where the
  cream reaches 4.5:1 on the blended fill (call made after the builder check).

- Fill: the face colour, always opaque. "Isi notch / Opasitas" rows are faded with
  the note "Jadi muka selalu solid".
- Face marks are drawn on the notch in place of the head, centred in the head's
  slot area (left of the clock), at about **1.6x the shrunk pill's size** (grid
  unit ≈ 1.28 dip). Every expression is drawn as marks: idle pills and blink,
  happy arcs, alert dots, sleep arcs with the z's beside them, the wave's happy
  arcs with the small heart beside them, focus, sad, sing.
- Falk cheeks and beak, octopus spots, calico patches (big circles clipped by the
  notch, top-left orange and bottom dark, as on the head) and the Black cat's rim
  as a 1.5 dip edge along the sides and bottom. Pale faces (White, Calico, falk
  WhiteFace) get the 1 dip surface-colour edge the pill already uses.
- Motion: look at the pointer slides the eyes only (no head), clamped inside the
  notch like `eyes_offset_in`; blinking as today; in the music views the face
  nods on the beat instead of the head.
- Ink: a per-view palette override for these views. Text, secondary and icon ink
  become #17120F when that holds 4.5:1 on the face, else stay cream; the caption
  colour is held at 4.5:1. Accent-coloured marks under 3:1 use the ink; the
  recording dot and level groups get the ring rule above. (Known case: red
  octopus #D9505F holds 4.66:1 for dark text; its caption colour must be tuned.)
- Accessories, all drawn inside the notch shape:

| Accessory | In Jadi muka |
|---|---|
| Kacamata | on the eyes, follows the look |
| Headphone | two earcups on the notch's left and right sides (the band is hidden) |
| Dasi kupu-kupu | small, under the eyes |
| Syal | a band along the notch's bottom edge with the short tail |
| Pita, Bunga | the notch's left corner |
| Topi tidur, Mahkota, Topi pelaut | **pinggiran** (the owner switched from miring di pojok on 2026-11-17: "yang topi di jadi muka, ganti deh mau pake cara jadi muka: pinggiran"): the hat is worn, its top past the screen edge, so only its lower part shows along the notch's top edge over the face, in every view, clear of the eyes and left of the clock, plus one detail. Topi tidur: navy band, cream fur under it, the tip flopping down the left of the face to a cream pompom. Mahkota: gold band with a zig-zag lower edge, three jewels (raspberry, blue, raspberry) and a darker gold edge (#9A6A12) so it reads on yellow. Topi pelaut: white band with a navy ribbon stripe, a navy rim under it and two short navy ribbon tails on the left. The hanging detail shrinks, or is left off, where there is no room left of the eyes (the narrow views) |

- Accessory contrast (call made after the builder check): a relocated piece whose
  main colour is under 3:1 against the face (for example the bow or scarf on a
  same-coloured face, the cream nightcap band on a cream face) gets a 1 dip edge
  in the face's eye colour, like the crown's gold edge. Tested for every
  accessory x coat.
- The shrunk pill: same fill, so the morph only shrinks the shape and slides and
  scales the face from 1.6x to 1x; the clock fades. Accessories are hidden on the
  pill.
- Things with no place in Jadi muka:
  - **Mascot stories (play)**: off while Jadi muka is on (they draw the full
    mascot around the island).
  - **Typing** (CLI working, laptop and paws): the face shows the focus eyes
    looking down instead.

## Left to the builder

- Brush handling (per-frame gradient vs a small cache keyed by height, stops and
  alpha), as long as idle draws add no timer or clock.
- Exact geometry of the face area per compact view and whether `robot_extra`
  widths need retuning, as long as the clock and titles keep their room and the
  face, its marks and every relocated accessory stay inside the shape for every
  notch size (160-300 x 28-44) and look offset (extend the clamp tests).
- How the palette override is threaded (the recon suggests a
  `theme::notch_palette(base, fill, style)` applied in `paint_island` after the
  panel early return), the dirty-rect signature fields and the tests.
- Wording of the description line and the faded-row note, in both languages.

## Done means

Merged into `main`, `cargo test --locked` green on `main`, new tests for the
contrast rules per coat and both styles, the clamp tests for Jadi muka, the
setting round trip and transfer classification; memory notes and README updated;
`main` pushed; worktrees cleaned; a list of what the owner has to check by hand
(real pointer look and shrink, bright wallpaper, each coat in both styles).
