---
tags: [decision, settings, layout]
date: 2026-10-27
---

# Settings: Tampilan scrolls

> Since v3.8 Settings has eight sections and Bata's rows (cat, animation,
> sleep, greeting) live on the Kucing page, not in Tampilan; see
> [[settings-structure]]. The scroll mechanics below still hold; which
> sections scroll is now listed there.

The Appearance page was full (preview + 5 rows end at y 452 of the 496
margin in a 520 px client). Bata's four rows (Asisten di notch, Animasi,
Tidur setelah, Sapaan pagi) go in a second card under it, so the section
scrolls instead of growing the window.

- `settings_layout::layout` lays the section out unscrolled; `scroll()`
  then shifts its content (not the sidebar or title) by `State::scroll`,
  clamped to `scroll_max` (what runs past the bottom margin), and sets a
  `viewport` from `VIEW_TOP` (60) to the window's bottom.
- `Layout::hits` cuts every control's rect to the viewport, so a control
  scrolled under the title or below the window takes no click.
- The painter clips the content to the viewport and draws a thin
  scrollbar thumb (`Layout::scrollbar`).
- `WM_MOUSEWHEEL` scrolls one row (52 px) per notch (`scroll_by`); a
  section change resets it. Appearance, General (its second card holds
  the disk / RAM warnings, [[system-page]]) and Clipboard (its second card,
  under a "Screenshot" eyebrow, holds the screenshot preview,
  [[screenshot-preview]]) have a viewport; every other section fits
  without one. The tests assert that, and that at the end the last card
  meets the margin.

The robot rows fade and take no clicks when the robot is off (`enabled`
on `Toggle` / `Segmented`, the stepper's existing flag); the sleep stepper
is also off for Mati. The design's "Hemat: tanpa kedip dan gerak, ekspresi
tetap" is too wide for one line beside the three segments with the
layout's width estimate, so it wraps (`Builder::wrap`, `row_text_lines`)
and the Animasi row grows by `DESC_LINE_H` per extra line
(`wrapped_row_h`). Do not shorten design copy to fit; wrap it.

**Notch card (v3.5):** a card of five rows (width slider, height slider,
"Ukuran bawaan" with Reset, "Latar notch" Hitam/Transparan, opacity slider)
sits between the first card and Bata's, so Bata's card is lower and the section
scrolls further. At scroll 0 only the width slider's row peeks into the window
(the scroll test asserts that, no longer the robot toggle). The sliders are the
section's first drag control besides the list handles. Details in
[[notch-size-and-fill]].

**v3.8 update:** Tampilan no longer holds the cat's rows or the two lists. The
cat moved to its own page (Kucing), the lists and Mode buka to Panel, and
Tampilan is the preview plus three titled cards (Island, Notch, Jendela); it
still scrolls (the preview, 4 + 5 + 1 rows, three eyebrows). The sections that
scroll are now Tampilan, Kucing, Panel, Clipboard and Fitur; Umum fits to the
pixel (one extra line when its search-hotkey label wraps) and Integrasi and
Tentang never scroll. The rest of this note describes how the first scrolling
section worked; the section list, "Bata's four rows" (now the Kucing page's
Gerak card, Animasi / Tidur setelah / Ikuti kursor / Mengecil jadi mata) and
the scroll test are in [[settings-structure]].

The Island card has five rows since "Equalizer ikut irama" was added
([[eq-follow-audio]]), so at scroll 0 the window ends inside that card and no
Notch slider peeks in (the scroll test now asserts the new toggle is visible
and nothing of the Notch card is).

**Resizable window:** `scroll_max` is the content's bottom minus
`client_h - PAD` and the viewport is `client_h - VIEW_TOP` high, both from the
real client size (`layout_in`, [[settings-window-size]]); a section that fits
gets no viewport. The scroll is re-clamped on every resize. The wheel also
scrolls while a list row is dragged ([[shortcut-tiles]]).

Related: [[bata-robot-expressions]], [[bata-idle-power-budget]],
[[notch-size-and-fill]], [[settings-structure]]

Part of [[v3-studio-overview]]. Listed in [[index]].
