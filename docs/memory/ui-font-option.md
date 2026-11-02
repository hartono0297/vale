---
tags: [decision, fonts, settings, config]
date: 2026-11-02
---

# UI font option: DM Sans or JetBrains Mono

Settings -> Tampilan -> Island -> **Font** ("DM Sans" | "JetBrains Mono"),
the row right after "Warna dasar" (the Island card has six rows). It is its
own option: not tied to the surface, no presets. Fonts and subsetting:
[[studio-font-sources]]; roles, tracking and the Mono sizes:
[[studio-font-loading]].

## Config

- Key `font` in config.json, `UiFont` (`Sans` default, `Mono`) in
  `src/config.rs` (a `named_enum!`, the last field of `Config`). Written as
  "Sans" / "Mono".
- Lenient (`ui_font`): a missing key, a typo, a number, `null` or an object
  reads Sans and the rest of the file is kept. Names are case-insensitive and
  trimmed ("mono", " MONO ").
- Island-owned in `merge_owned` (`font: mem.font`), like the surface and the
  accent: the island's copy wins. A hand edit of `font` made while the island
  runs does **not** apply live (the file is not watched, and `reload_config`,
  whose only caller is `connect_google`, merges the island's font back in),
  and the next `save_config` writes the island's font over it. Edit the file
  with the island stopped; it is read at startup.

## Live switch path

1. A click on a segment posts `Action::Font(UiFont)` (`settings.rs`, as every
   other segment); `on_settings_action` sets `config.font` and `save_config`s.
2. `adopt_config` ends in `App::apply_ui_font`: compared with
   `Renderer::ui_font()` (what is in effect, so a caller may set the field
   first). It calls `Renderer::set_ui_font` (rebuilds the formats only when the
   font changed; tracking follows `fonts::display_tracking_em`), re-measures
   the lyric line on screen (`fit_lyric_line`, see [[lyric-notch-width]]; the
   notch is as wide as its line), then `refresh` and `draw`.
3. `sync_settings` pushes the model back: the open Settings window's
   `on_model` calls `Gfx::set_font` (formats rebuilt, the width cache
   cleared), gives the native EDIT fields a new GDI font
   (`gdi_text_face_for`) and lays out again. The window and the island switch
   together, with no restart.
4. At startup `renderer.set_ui_font(config.font)` runs before the first draw.

## Scope

- **Editors stay DM Sans**: the reminder and screenshot editors
  (`editor.rs`, `editor_paint.rs`, `shot_edit/*`) call `Face::for_role`,
  `gdi_text_face()` and `apply_tracking`, which are the Sans wrappers.
- **Code stays DM Mono** (`Role::Mono`) in both.
- CJK and emoji stay on DirectWrite's system fallback.
- Not changed: media buttons and commands, the cat, accents, logo, tray, the
  surface list.

## Demo

`DI_FONT=mono` (`UiFont::from_name`; unset or unknown is Sans) applies to
every `--demo*` scene that builds a `Renderer` (`run_scenes`) and to
`--settings` (read just before `let model = Model {`). Documented in the
`demo.rs` header. See [[screenshot-capture-script]] before capturing and
[[demo-data-time-of-day]].

Related: [[settings-structure]], [[exe-size]], [[cat-colours-and-surfaces]]

Listed in [[index]].
