---
tags: [decision, settings, layout, v3.8]
date: 2026-10-31
---

# Settings: nine sections, titled cards (v3.8, Tombol pintas added later)

The window used to have six sections with Tampilan holding half the
settings. v3.8 sorted them into eight (`Section`, `settings_layout.rs`); the
recordable hotkeys added a ninth, **Tombol pintas** / Hotkeys, right after
Clipboard ([[hotkeys]]). The sidebar order is the table's. `--settings <name>`
takes the English or the Indonesian name (`from_name`): general / umum,
appearance / tampilan, cat / kucing, panel, clipboard, hotkeys /
tombol-pintas / tombolpintas / shortcut-keys (not "shortcuts": the tiles are
Pintasan), features / fitur (also the old `pomodoro`), integrations /
integrasi, about / tentang. The window opens on Umum.

| Section | Cards (eyebrow titles) |
|---|---|
| Umum | **Dasar**: Bahasa, Jalan saat Windows mulai (2 rows since the search hotkey moved to Tombol pintas). **Peringatan**: Baterai (off on a PC without one), Disk, RAM, Popup Windows with "Buka petunjuk" |
| Tampilan | the preview; **Island**: Gaya island, Warna aksen, Warna dasar (swatches), Jam saat kosong, Equalizer ikut irama ([[eq-follow-audio]]). **Notch**: Lebar, Tinggi, Ukuran bawaan, Isi notch (Solid / Tembus), Opasitas. **Jendela**: Mode jendela |
| Kucing | the preview (cat about 64 px on the resolved surface, the idle notch at real size with the clock, chips Diam / Senang / Kedip / Tidur / Menyapa); **Kucing**: Kucing di notch, Warna, "Latar: <name>" with "Ubah", Aksesori tiles; **Gerak**: Animasi, Tidur setelah, Ikuti kursor, Joget ikut musik ([[cat-dances-to-music]]), Mengecil jadi mata; **Sapaan**: the toggle (one key, `CatGreeting`, with `MorningGreetingDesc` under it) and one read-only line of the five periods, built from `greeting.rs` (`DayPeriod::ALL`, `hours()`) and `i18n::greeting_title` by `i18n::greeting_periods_line`, so the hours cannot drift from `period_at` ([[greeting-periods]]) |
| Panel | **Perilaku**: Mode buka, Di belakang jendela. **Halaman panel**, **Pintasan**, **Tersedia** (the lists and the own-tile form, [[panel-pages]], [[shortcut-tiles]]) |
| Clipboard | **Riwayat** (7 rows: no hotkey row); **Screenshot** ([[screenshot-preview]]; 4 rows: tool, preview, on-ignore, seconds, no PrtSc row) |
| Tombol pintas | **Di semua aplikasi**: one card, a header (hint and "Kembalikan semua") and five recordable rows ([[hotkeys]]) |
| Fitur | **Pomodoro** (4 steppers and the running note), **Musik** (Lirik di notch), **Notifikasi** (Simpan notifikasi, Jumlah maksimal, Simpan setelah restart, Aplikasi dikecualikan with a remove button each and a field with *Tambah*, Hapus riwayat notifikasi with two clicks; [[notification-history]]), **CLI** (Folder awal, Shell), **Perangkat** (Notifikasi perangkat; one row), **Susun** (gap slider first, then the templates and saved lists), **Pembersih** (Lindungi cache Cargo & Autodesk di Mole) |
| Integrasi, Tentang | the same content, each card under a title (Tentang's is "Tautan": 4 rows since [[self-update]], Kode sumber, Pembaruan with "Cek sekarang" / "Update", the toggle "Cek pembaruan otomatis", Folder data) |

Every card has an eyebrow (`titled` / `Builder::eyebrow`) except the two
previews, which are pictures; `every_card_of_every_section_has_a_title_but_the_previews`
checks it. Every row keeps its exact behaviour: the same `Hit`, `Action` and
`SettingsAction`.

## New and renamed rows

- Labels: "Latar island" is "Warna dasar", "Latar notch" Hitam | Transparan is
  "Isi notch" Solid | Tembus ("Notch fill", "See-through"), "Opasitas latar"
  is "Opasitas", "Asisten di notch" is "Kucing di notch", "Sapaan pagi" is
  "Sapaan" (`CatGreeting`; the old `MorningGreeting` key is gone, the description is unchanged), "Notice
  perangkat" is "Notifikasi perangkat", "Notifikasi salinan" is "Pratinjau
  salinan", "Disalin untuk
  semua salinan" says "Hanya saat pratinjau mati", "Preview di notch" is
  "Pratinjau di notch". "Alat screenshot" has a description. (The "Tombol
  pintas" rows of Umum, Clipboard, Perangkat and Susun, and "Tombol PrtSc",
  moved into the Tombol pintas section, [[hotkeys]].)
  English labels match.
- **Warna dasar** are swatches: Ikuti kucing (Auto, drawn as the colour it
  gives now with a small cat outline), Espresso, Graphite, Laut, Malam, Ungu.
  The row's description is the chosen name; Auto adds what it gives
  ("Ikuti kucing · Laut"). Kucing has no second picker: "Ubah" jumps to
  Tampilan.
- **Kucing preview** is still: no timer and no animation in Settings. The
  chips only change `Settings::cat_face` (window state, never saved).
- **While the cat is off**, Warna, Latar, Aksesori, Animasi, Tidur setelah,
  Ikuti kursor, Joget ikut musik and Sapaan fade and take no clicks (`enabled`);
  **Mengecil jadi mata** does not need the cat (the eyes show without it), but
  is off, with a note, while Di belakang jendela is on ([[notch-peek-eyes]]).
- The battery stepper is off when the power status says the PC has none
  (`sources::battery::battery_present`: `BatteryFlag` 128; 255, unknown,
  counts as present).
- The accessory tiles and the preview draw the cat with `RobotPainter` in the
  window's own render target ([[cat-accessories]]).
- Icons: a new cat-head outline (`Icon::Cat`) is the Kucing sidebar icon;
  Panel uses `Grid`, Fitur `Sparkle`.

## Scrolling ([[settings-appearance-scroll]])

The window is no longer fixed at 520 px high ([[settings-window-size]]): the
statements below hold at the 720x520 minimum client size, which is what the
layout tests use. A bigger window needs less scrolling and a tall enough one
needs no viewport at all (`scroll_max` is 0 and the scrollbar goes).

Only a section that overflows gets a viewport. Tampilan, Kucing, Panel,
Clipboard and Fitur always do; Umum, Tombol pintas, Integrasi and Tentang
never. Umum used to fit the 520 px window to the pixel and trim its gaps when
the search-hotkey label wrapped (`UMUM_TRIM_*`, `LABEL_LINE_H`); with that row
gone to Tombol pintas it is 2 + 4 rows and never wraps, so all that logic was
deleted. Tombol pintas is one card of a 48 px header and five rows, well
inside the window.
`exactly_the_overflowing_sections_get_a_viewport` and
`everything_fits_in_the_window_height` assert it, and that the last card meets
the bottom margin when scrolled to the end.

## Fitur: Notifikasi card (v3.9)

A titled card right after Musik ([[notification-history]]): *Simpan notifikasi*
(toggle), *Jumlah maksimal* (stepper 10-200 by 10), *Simpan setelah restart*
(toggle), *Aplikasi dikecualikan* (the `muted_apps` list: a row each with
*Hapus*, "Belum ada yang dikecualikan" when empty, then a field with
*Tambah*) and *Hapus riwayat notifikasi* (two clicks, the clipboard's
`TIMER_CONFIRM` pattern, sent to the island as `SettingsAction::NotifClear`).
The add field **reuses the own-shortcut form's Nama EDIT** (`Field::TileName`):
only one section shows at a time, so Fitur lays that field out in its add row
(Enter adds, Esc empties it, its hint reads "Nama aplikasi") and entering or
leaving Fitur empties it, so what was typed for a tile never lands in the
exclusions. Adding trims, cuts to 64 characters and skips a repeat in any
case (`config::with_muted`); every edit goes through
`save_config_with(|c| c.muted_apps = list)`, so hand edits of other fields
survive, and the island pushes the model back (`sync_settings`) so an open
Settings window follows an exclusion made from the page's row. Fitur already
scrolled; the extra rows only lengthen it.

Related: [[cat-colours-and-surfaces]], [[bata-idle-power-budget]]
