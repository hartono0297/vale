---
tags: [fonts, directwrite, gotcha]
date: 2026-11-02
---

# Font loading: DirectWrite and GDI

Implemented in `src/ui/fonts.rs`; sources and subsetting in [[studio-font-sources]].
The UI typeface is an option (DM Sans or JetBrains Mono): [[ui-font-option]].

- Never unregister the in-memory font loader while text formats created from its collection are alive.
- `DrawText` cannot carry OpenType features. Text that needs tabular figures (`tnum`) must go through a text layout; the renderers share one tabular-figures setting.
- Missing or broken fonts fall back (Segoe UI Variable, Cascadia Mono / Consolas) without crashing; `choose_in(role, ui, loaded, installed)` is the pure selector (`choose` is its Sans wrapper).
- GDI windows use `AddFontMemResourceEx` via `register_gdi()` and then verify that GDI resolves the face (DM Sans, DM Mono, JetBrains Mono: `GdiFonts`).
- The display role (titles, large clocks) is DM Sans 600 upright with letter-spacing -2.5%, applied by `fonts::apply_tracking` (`IDWriteTextLayout1::SetCharacterSpacing`, constant `fonts::DISPLAY_TRACKING_EM`). Apply it in both the draw and the measure path, or measured widths will not match what is drawn. Like `tnum` it needs a text layout: `DrawText` cannot carry it. DM Sans tops out at weight 600.
- The layout functions are pure and have no fonts (the calendar title is the exception: the renderer measures it and passes the width in, see [[studio-calendar-day-view]]), so other boxes that hold display text are sized from widths measured once with the real fonts and kept as constants beside a test (the pomodoro ring fits "100:00" in DM Sans 600 at 48 px; the idle clock box fits "00:00" in DM Sans 600 at 16 px). Re-measure when a font file, size or tracking changes.
- Digits: [[studio-tabular-figures-word-gaps]].
- The lyric notch is sized from `Renderer::lyric_text_width`, the same layout the line is drawn with: [[lyric-notch-width]].

## Roles per UI font (`UiFont`, v3.13)

| Role | Sans (default) | Mono |
|---|---|---|
| `Text` | DM Sans | JetBrains Mono when loaded, else DM Sans when loaded, else the Segoe chain |
| `Display` (600) | DM Sans 600, tracked -2.5 % | JetBrains Mono 600, **no tracking** |
| `Mono` (code) | DM Mono | DM Mono |

`Face::for_role_in(dwrite, role, ui)` and `choose_in` are the selectors;
`Face::for_role`, `choose` and `gdi_text_face()` stay as the Sans wrappers
because the reminder and screenshot editors stay DM Sans.

- **No tracking for Mono.** A monospace cell is designed spacing: JetBrains
  Mono is 0.6 em a character, and negative tracking cramps the cells so that
  letters touch. `fonts::display_tracking_em(ui)` gives -0.025 for Sans and 0
  for Mono, and `apply_tracking_in` applies nothing at 0. The renderer's
  `layout()` and Settings' `Gfx::line` (draw and measure) both go through it.
- **Tabular figures** are unneeded for JetBrains Mono (every glyph is one
  cell), but the digit-run-only `tnum` path stays; a test checks that text
  without digits measures the same with and without the typography and that a
  space is one cell.
- **No ligatures**: the JetBrains Mono subset has no `calt`, `liga` or `dlig`
  ([[studio-font-sources]]), so "->", "!=", "==", "www" are drawn literally by
  DirectWrite and GDI.
- **GDI**: `gdi_text_face_for(ui)` is "JetBrains Mono" for Mono when GDI
  resolved it, else `gdi_text_face()`. Settings' EDIT fields get a new HFONT
  on a font change.

### Mono sizes and widths

Layout constants are font-agnostic and the Sans sizes are unchanged. Only the
displays in boxes sized from DM Sans widths get a Mono size
(`DisplaySizes::MONO` in `render.rs`), the largest 0.5 px step that fits with
the ink inside, measured with the real embedded JetBrains Mono v2.304
(`mono_display_sizes_are_the_largest_that_fit` pins them and the widths):

| Text | Box | Sans | Mono |
|---|---|---|---|
| Idle clock "00:00" | `CLOCK_W` 44 | 16 px, 40.3 px wide | 14.5 px, 43.5 px |
| Pomodoro "100:00" | the ring's time box, 167.5 px | 48 px, 148.6 px | 46.5 px, 167.4 px (47 does not fit) |
| Page titles | measured live / wider than any title | 20 px | 20 px ("Oktober 2026" is 144 px, 12 cells; `CLI_TITLE_W` 56 holds "CLI" at 36 px) |
| Lyric, song title, System % | live / wide boxes | 16 / 28 / 30 | unchanged |

`display_text_fits_its_boxes` runs the clock, the countdown, the CLI title,
the calendar titles and the System figure for both fonts. The notice chips
(`ACTION_W` 66, `LOG_ACTION_W` 46, `RELEASES_ACTION_W` 88) hold their words in
both languages and both fonts. Headers, chips and most buttons measure their
text live. Other fixed columns are constants sized for DM Sans and are only
checked by arithmetic for Mono (`METER_NAME_W`, `PROC_*_W`, `CLEAN_SIZE_W`,
`DEV_DETAIL_LABEL_W`, `VIEW_BUTTON_W`); check them with the real font when
their text or size changes. The Notifikasi time column is the one that did
not fit: "28 Okt 2025" (an entry from an earlier year) is 11 cells, 69.3 px
in `small_right` (10.5 px), over `NOTIF_TIME_W` 64. It is now
`NotifHead::time_w`, measured from `notif::time::longest_samples` (the
longest line of each `relative_time` rule) and never under 64: DM Sans stays
64 ("28 May 2025" is 63.8 px), Mono gets 70.
`notif_time_column_holds_every_relative_time_in_both_fonts` checks it. Mono
also trims where a notch is genuinely too narrow (the compact notch's song
title gets its "..." sooner). Re-measure the Mono sizes when the font file, a
size or the box changes.

`Estimate` in `settings_layout.rs` keeps its factors (0.62, 0.66 for weight
500 and up, 0.8 for the eyebrow): JetBrains Mono is 0.6 em, below them, and
`real_jetbrains_mono_is_within_the_estimate` measures every label of every
section with the real font. The sidebar brand rect holds "Vale" in Mono 16
px 600 (38.4 px).

Part of [[v3-studio-overview]].
- Tracking adds its spacing after every character, so centered display text (clock, lyric, big time) sits about `size * 0.025 / 2` px off center (0.6 px at 48 px). Accepted, not corrected.
- `settings_layout::Estimate` has no special display factor any more: display fonts use the 0.66 em/char estimate for weight 500+, which is wider than real DM Sans 600. The sidebar brand rect was widened to the gutter so the estimate fits; keep that in mind before shrinking it.
