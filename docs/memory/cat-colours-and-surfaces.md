---
tags: [decision, cat, theme, settings, v3.8]
date: 2026-11-03
---

# Cat colours and island surfaces (v3.8)

The notch cat has four coats and the island has five fills plus "follow the
cat". Pure data in `src/theme.rs` (`CatColor`, `CatAccessory`, `Surface`) and
`src/ui/cat_style.rs` (`CatLook`); config keys in [[settings-structure]]'s
Kucing page. The accessories and the paint order are in [[cat-accessories]].

## Coats (`CatLook`)

| Coat | Head and ears | Eyes and every eye mark | Extra |
|---|---|---|---|
| Terracotta (default) | #E2683C | #17120F | as before v3.8 |
| Calico | #F4EDE4 | #17120F | as the landing page cat: left ear orange #E2683C, right ear dark #2B2420, an orange circle top-left and a dark circle bottom-right on the head, cream elsewhere. The Settings swatch is the site chip's conic: right half cream, bottom-left quarter orange, top-left quarter dark (`calico_swatch_slices`) |
| White | #F6F1EA | #17120F | |
| Black | #1E1B22 | amber #F2B33D | a light rim #8E7CB8, 1.6 grid units round head and ears |

"Every eye mark" = the open pills, the alert dots, the blink lines, the
happy and sleep arcs (`Ink::Espresso` becomes the coat's eye colour). The
extras beside the head (z's, notes, thought dots, the heart) stay cream.
Not routed through the coat: the app logo, exe icon, tray, README and the
Settings sidebar logo (they stay terracotta, see [[logo]]), and the
notch-peek eyes pill ([[notch-peek-eyes]]). The see-through legibility halo
([[notch-size-and-fill]]) still sits behind the head of every coat.

## Surfaces

`Surface { Auto, Espresso, Graphite, Sea, Night, Plum, Terminal }`, `Auto` the default
(Terminal was appended last, so the `pack` indices keep their meaning).
A file without `surface`, or with an unknown name, reads as Auto; a stored
"Espresso" or "Graphite" keeps its meaning, and the old "Black" still reads
as Espresso. Fills: Espresso #17120F and Graphite #221B17 unchanged, Sea
#132321, Night #151C2E, Plum #2A2140, Terminal #1B1D1F (a flat neutral
near-black with no warm tint; label "Terminal" in both languages).

`Surface::resolve(cat, robot_on)`: Auto gives Terracotta -> Espresso,
Calico -> Sea, White -> Night, Black -> Plum, and Espresso with the cat off;
any other value is itself. **Auto never reaches `island()`**:
`island_palette(cfg)` and `Theme::of` resolve first (`island` itself reads a
stray Auto as Espresso), and the Settings preview resolves the same way.
`theme::pack` / `unpack` carry the seven surfaces (a test walks every
combination, and Terminal has its own round trip).

Auto never resolves to Terminal (a test checks it for every cat, robot on or off).

Terminal also makes the dark windows (Settings, the editors, the screenshot
editor) neutral instead of warm brown: [[terminal-windows]].

The coloured surfaces lift their own colour the way Graphite lifts Espresso:
raised +12, raised_hi +34, track and placeholder +26, column +5, dot_off +50
on every channel (Espresso and Graphite keep their pinned values). On
Terminal that gives raised #27292B, raised_hi #3D3F41, track and placeholder
#353739, column #202224, dot_off #4D4F51: each stays within a spread of 4
between channels, like the #1B1D1F source, so the surface stays neutral. Text stays
#F4EDE4; each surface has its own `secondary` caption colour, because
#A89A8B drops to about 4.2:1 on the coloured fills.

## Contrast (WCAG, measured by `every_concrete_surface_keeps_text_and_secondary_readable`)

| Surface | Text on surface / raised | Secondary | Secondary on surface / raised |
|---|---|---|---|
| Espresso #17120F | 16.00 / 14.30 | #A89A8B | 6.78 / 6.06 |
| Graphite #221B17 | 14.61 / 12.78 | #A89A8B | 6.19 / 5.42 |
| Sea #132321 | 14.02 / 12.03 | #A3B8B2 | 7.80 / 6.69 |
| Night #151C2E | 14.61 / 12.65 | #A7B1C8 | 7.89 / 6.83 |
| Plum #2A2140 | 13.01 / 11.05 | #B9ACD0 | 7.10 / 6.03 |
| Terminal #1B1D1F | 14.56 / 12.57 | #A9ADB2 (neutral grey) | 7.49 / 6.47 |

The ink on every accent stays at least 4.5:1. The status colours (sage,
ochre, plum) hold 4.5:1 on every surface; the third event colour #B07A9A fell
to 4.39:1 on the Plum surface, so the Plum surface uses #C893B4 (5.96:1 on
the surface, 5.06:1 on raised) for it. Terminal does the same: #B07A9A is
4.91:1 on its fill but 4.21:1 on raised (it is under 4.5:1 on the raised
step of Graphite, Sea and Night too, which the test does not check for
the default third colour), so Terminal uses #C893B4 (6.67:1 / 5.76:1).
Sage and ochre hold at least 6.4:1 on Terminal's surface and raised.

## Config (lenient, island-owned)

`cat_color`, `cat_accessory`, `cat_follow_pointer`, `notch_peek` sit at the
end of `Config`, all in `merge_owned`. A bad value falls back per field (a
typo, a number, null) and never drops the file; the two bools read on unless
a real `false`. Old files load unchanged.

## Demo

`DI_CAT_COLOR`, `DI_CAT_ACCESSORY` and `DI_SURFACE` apply to every `--demo`
scene (an unset or `auto` surface follows the cat, as in the app); the scenes
`bata-colors` and `bata-accessories` are in `--demo-robot`.

- `bata-colors`: with `DI_SURFACE` unset or `auto`, each coat sits on the
  surface Auto gives it; with a concrete surface (say `graphite`) every coat's
  swatch uses that one (the unresolved choice travels in `SheetKind::Colors`
  and is resolved per coat, which gives the same surface each time).
- `bata-accessories`: draws on the demo's own palette (`self.palette`, so
  `DI_SURFACE` and `DI_ACCENT` count). With no env vars that is Auto resolved
  from `DI_CAT_COLOR`, as before.

Related: [[bata-robot-expressions]], [[settings-structure]]

## Other mascots (2026-11-13)

`Surface::resolve` takes a `theme::Coat` now (`Coat::of(cfg)` is the chosen
mascot's own coat). Auto for the falk: Lutino → Espresso, Grey → Night,
WhiteFace → Plum; for the octopus: Coral → Espresso, Red → Graphite,
Purple → Night, Blue → Sea. The cat's rows above are unchanged. See
[[maskot-falk-gurita]].
