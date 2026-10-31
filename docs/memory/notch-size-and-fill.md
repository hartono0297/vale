---
tags: [decision, settings, geometry, rendering]
date: 2026-10-30
---

# Notch size and see-through background

> Since v3.8 the card is titled "Notch" and its fill row is "Isi notch"
> (Solid | Tembus, was "Latar notch" Hitam | Transparan); the island colour
> is "Warna dasar" with five surfaces plus Auto. See [[settings-structure]]
> and [[cat-colours-and-surfaces]]. Size, alpha and drag behaviour below are
> unchanged.

Vale v3.5 requests #1 and #4, together in one new card in Settings ->
Tampilan ("Notch", between the first card and Bata's). Config (island-owned,
in `merge_owned`), geometry in `src/ui/geometry.rs`, alpha math in
`src/ui/notch_fill.rs`, UI in `settings_layout.rs` / `settings_controls.rs` /
`settings.rs`, wiring in `App` (`set_notch_size`, `fill_at`).

## Size

- `notch_width` 160..=300 (default 190), `notch_height` 28..=44 (default 34),
  consts `NOTCH_W_MIN/MAX/DEFAULT`, `NOTCH_H_MIN/MAX/DEFAULT` in `geometry.rs`.
  Lenient deserializers (non-number -> default, out of range -> clamped), like
  `disk_warn`.
- Why those limits: 160 keeps idle robot + clock and the compact media title
  (still ~77 px) inside the shape; 28 is Bata's 26 px plus 1 px each side
  (Notch corner radius 16 still fits: `corner_radius` keeps `h - SHOULDER`);
  44 stays well under the Notice's 76, so a notice still grows from it.
- `NotchSize { w, h }` (`DEFAULT` = 190 x 34, `from_px`, `from_config`),
  `shape_sized`, `shape_with_robot_sized`, `lyric_shape_sized`,
  `lyric_width_min`. `shape_for`, `shape_with_robot`, `lyric_shape`,
  `lyric_width` are the default-size forms (the last three are test-only now).
- Follow the size: Idle (w x h), MediaCompact (w + 12), PomodoroCompact (w + 22; see
  [[compact-padding]]), and
  MediaLyric (height; its minimum width is `max(LYRIC_MIN_W, w)`, and
  `App::lyric_max_w` never goes below it, `LyricFit::set_min`). Notice,
  NoticeShot and every panel kind do not. See [[lyric-notch-width]].
- `motion::FadeStart::compact_h` replaces the fixed `COMPACT_H` in
  `fade_floor` (the constant stays as the default for tests and `Transit`).
  `App::fade_floor_h` starts at the notch height. See
  [[panel-linger-and-motion]].
- Live drag vs commit: the slider posts `Action::NotchSize { commit: false }`
  only when the whole number changes; `App::set_notch_size(.., live: true)`
  settles the anim on the new shape (`Anim::settled`, no morph) so the island
  follows the pointer, nothing is saved. Release (or capture lost) posts
  `commit: true`: `refresh` (a no-op morph, already there) and one
  `save_config`. Reset and the fill choice save at once. The window keeps the
  dragged value in its own model and ignores an echo of an older step while the
  thumb is held (`Settings::slider`, `update`).
- Hidden: the anim is settled on the new Idle shape so the next appearance
  grows from it.

## Background (Hitam / Transparan)

- `notch_fill: NotchFill { Solid, Transparent }` (typo -> Solid) and
  `notch_opacity` 0..=100 (default 30). Old files load unchanged; the Solid
  path draws exactly as before (alpha 1.0, same fill call, no shadow draws).
- Scope: the compact views and notices. The open panel always keeps its solid
  surface. `Frame::fill_alpha` / `Frame::legibility` carry the numbers.
- `FILL_FLOOR` = 3/255 (8-bit alpha 3): a layered window only takes the pointer
  on pixels with alpha > 0, so a 0 % notch must stay hoverable and clickable.
  Same idea as `HALO_ALPHA` in `render.rs` (see [[shelf]]); invisible.
- Morphs: `App::fill` = (from, to). Opening a panel follows the eased progress
  (`morph_fill_alpha`, clamped 0..1, so the overshoot never leaves the range);
  closing into a thinner fill waits for the last 30 % (`closing_fill_t`) so the
  leaving panel's content keeps its solid fill and nothing pops. Retargeting
  mid-morph starts from the alpha showing now.
- Legibility (`notch_fill::legibility`): 0 from alpha 0.85 up, rising to ~1 at
  the floor. `Renderer::legible` (a `Cell`) is set only around the compact and
  notice painters; `text_legible` draws a 4-direction 1 px black outline
  (0.35) and a black copy one px lower (0.55), both x strength; icons,
  equalizer bars and Bata (a soft dark rounded halo behind the head) get the
  same treatment. Chips, badges, shot buttons, the placeholder art tile and
  the volume plate keep their own fills and switch it off (`solid_ink`).
- Preview card in Settings draws the island at the set size, with a checker
  strip behind it when see-through.

**Labels since v3.8** ([[settings-structure]]): the card is titled "Notch" in
Tampilan (the preview, then Island, Notch, Jendela); "Latar notch" Hitam |
Transparan is now **"Isi notch"** Solid | Tembus (EN "Notch fill", Solid |
See-through) and "Opasitas latar" is "Opasitas", still faded unless Tembus.
Config values (`notch_fill`: Solid or Transparent) did not change.

## Settings UI

`Control::Slider` + `Hit::Slider(SliderId)` (`NotchWidth`, `NotchHeight`,
`NotchOpacity`), pure `slider_value` / `slider_x` (round trip tested). A press
jumps the thumb and starts a drag with `SetCapture` (same pattern as the list
handles), `WM_LBUTTONUP` / `WM_CAPTURECHANGED` commit. The opacity slider is
faded and inactive while Hitam. "Atur ulang" is off at 190 x 34. The card
adds five rows (260 px), so [[settings-appearance-scroll]]'s Bata card moved
down; at scroll 0 only the width slider peeks in.

## Demo

`DI_NOTCH=WxH`, `DI_NOTCH_FILL=transparent`, `DI_NOTCH_OPACITY=0..100` apply to
every `--demo` scene; scenes `idle-transparent` and `media-transparent`.

## Not verified live

- The drag feel (live follow, one save on release) and the cursor over the
  thumb were not tried with a real mouse; only the pure math, layout and the
  `--demo` / `--settings` runs (no crash, no draw error) were.
- How the shadow reads on a bright wallpaper, and how the checker preview
  looks, were not looked at on screen.
- A hand edit of the four fields while Vale runs is not picked up
  (island-owned: `merge_owned` keeps the in-memory values), as with `surface`.
- Arrow keys on a focused slider were left out.
