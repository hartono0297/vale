---
tags: [design, ui, tooltip]
date: 2026-11-12
---

# Tooltips on icon buttons (design)

## Why

The owner could not tell what the sticky note header buttons do (list, pin,
plus, check, colour, three dots, close): they are icons with no label. Their
words: "ditambahin fitur tooltips atau yang biasa kalo di hover ada
informasinya (kalo bisa cek lagi kesemua menu, tambahkan hover information)".
Asked what a tooltip should say, they answered "se informatif mungkin, kalau
bisa kalau dia ada hotkey, infokan hotkey nya juga dan untuk apa". They picked
the look that follows the window theme (option B of the mockup) and left the
rest of the behaviour to the builder ("u atur yang terbaiknya gimana").

## Goals

- One Vale tooltip, used by every own-drawn window that has icon-only buttons
  or buttons whose label does not say enough.
- It says: the name, the hotkey when there is one, and one line on what a
  click does.
- It looks like Vale in every theme (light, warm dark, Terminal), in both
  languages, on monitors with different DPI, and never leaves the screen.
- Zero cost at idle (see [[bata-idle-power-budget]]).

## Non-goals

- Tooltips on controls whose own text already says everything (text buttons
  such as "Simpan", "Batal", "+ Tambah"; text rows; switches with a label).
- Tooltips on the tray menu (native) or on the rows of Vale's dropdown.
- Changing what any button does.
- The screenshot inline strip, its action bar and the screen-recording picker
  keep their own in-overlay tips (they paint over a frozen fullscreen overlay
  and already name the button and its key). The screenshot editor window
  keeps its footer hint.

## Anti-goals (a failure even if it works)

- A tooltip that flickers, follows the pointer, or shows over a drag.
- A tooltip that steals focus, takes clicks, or makes the island lose its
  hover (the panel must not collapse or stay open because of it).
- A timer or thread that runs while nothing is hovered.
- A tooltip in a screen recording or in a screenshot Vale takes.
- Text that is wrong about what the button does, or a hotkey that is not the
  one in the owner's settings.

## Look (decided by the owner: option B)

- A rounded plate in the window palette (`theme::current_window()`): `popover`
  fill, 1 px `border` hairline, soft shadow as in `ui/menu_pop.rs`. Light,
  warm dark and Terminal follow the theme live.
- Line 1: the name in the text colour, weight 600, about 12.5 dip. The hotkey
  sits at the right end of line 1 in a small key plate (`button` fill, text
  colour, weight 600, about 10.5 dip). Several keys or a gesture are written
  as text in the plate ("Ctrl+Alt+L", "Esc", "Klik kanan").
- Line 2 (optional): what a click does, in the `secondary` colour, about
  12 dip, wrapping. The plate is at most 280 dip wide.
- Short form, for a button that already shows a label: no name line, only the
  description in the text colour plus the key plate.
- Contrast measured for the mockup: text 16.6:1 / 13.5:1, secondary 5.07:1
  (light) / 5.70:1 (warm dark) / 6.64:1 (Terminal, on `0x252729`). The build
  re-checks with the real palette values, key plate included.
- Font: the UI font setting (`ui/fonts`), like the dropdown.

## Behaviour (builder's call, owner delegated it)

- **Show:** the pointer rests on a button with a tip for 500 ms. While a tip
  is up, or within 300 ms after one went away, moving onto another button
  with a tip shows that one at once (no new wait).
- **Hide at once:** the pointer leaves the button, any mouse press, the
  wheel, a key press, a drag starts, the owner window hides, moves, resizes,
  or loses its region (panel collapsing, page sliding), the theme or language
  changes. After a click, that button shows no tip again until the pointer
  has left it.
- **Keyboard:** in windows that already move focus with Tab (transcript
  window, sticky note list, editors), an icon button that gets keyboard focus
  shows its tip after the same delay; it goes on the next key.
- **Place:** centred below the button with a 6 dip gap; above it when there is
  no room below in the work area of the button's monitor; pushed sideways to
  stay inside that work area. Anything anchored to the closed notch goes
  below. Size and gap use the DPI of the button's monitor.
- **Window:** one popup for the whole app, layered, topmost,
  `WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW`, so it never takes
  focus or clicks. It exists only while a tip is visible. The 500 ms wait and
  the 300 ms "warm" window are one-shot timers that exist only while the
  pointer is on a button with a tip or just after a tip went away.
- **Capture:** it is a Vale window, so it is out of the video while a screen
  recording runs (the existing in-context exclusion hook); a screenshot hides
  it first, like other transient Vale popups.
- **No animation:** it appears and goes without a fade. A tooltip is read in
  passing; a fade would only delay it and needs frame clock ticks.
- **Text source:** every string is an i18n `Key` (both languages, compile
  checked). Hotkeys are read from the live config at show time; a hotkey with
  no combo is not shown. A tip that names an app or a view is built by a
  small helper in `i18n.rs`.
- **API shape (builder's call):** a caller gives an anchor rect in screen px,
  the owner `HWND` and a `Tip { name: Option<&str>, key: Option<String>,
  about: Option<&str> }`, and calls a hover entry point on every pointer move
  with the current tip id (or none). The module owns the delay, the warm
  window, the "clicked, wait for leave" rule and placement; callers only
  report hover, press and leave.

## Where tips go, and what they say

Hotkey column: `H:Name` means the global hotkey `HotkeyId::Name` as set now
(shown only when it has a combo). Other keys are fixed. "—" means none.
Before using any text below, check it against what the code really does; if
a description is wrong, fix the text, not the behaviour.

### Sticky note header (`widgets/`, `Btn`)

| Button | Name (ID / EN) | About (ID / EN) | Key |
|---|---|---|---|
| List | Daftar catatan tempel / Sticky note list | Semua catatan tempel dalam satu jendela, bisa dicari. / Every sticky note in one window, with search. | H:StickyList |
| Pin, off | Sematkan di atas / Keep on top | Catatan ini tetap di atas jendela lain. / This note stays above other windows. | — |
| Pin, on | Lepas sematan / Stop keeping on top | Jendela lain bisa menutupi catatan ini lagi. / Other windows can cover this note again. | — |
| Plus | Catatan baru / New note | Catatan kosong di samping catatan ini. / A blank note next to this one. | — |
| Plus at the cap | Catatan baru / New note | the existing `StickyMax` text | — |
| Check | Daftar centang / Checklist | Ubah baris ini jadi kotak centang. / Turn this line into a checkbox. | Ctrl+Shift+9 |
| Colour | Warna dan gaya / Colour and style | Kertas, ukuran, huruf, garis, dan tembus pandang. / Paper, size, font, lines and see-through. | — |
| More | Lainnya / More | Pengingat, gulung, selip ke tepi, kirim ke HP, hapus. / Reminder, roll up, tuck to edge, send to phone, delete. | Klik kanan / Right-click |
| Close | Lepas dari desktop / Take off the desktop | Isinya tetap ada di halaman Catatan. / It stays on the Notes page. | — |
| Prev / Next (stack) | Lembar sebelumnya / Previous sheet; Lembar berikutnya / Next sheet | Roda mouse di strip atas juga bisa. / The wheel over the top strip works too. | — |
| Image ✕ | Hapus gambar / Remove image (existing `FmtRemoveImage`) | Gambar dikeluarkan dari catatan ini. / The image is taken out of this note. | — |

Format bar (`Tool`): moves to the shared tooltip, same names and shortcuts as
now, no description, except: Marker's chevron half "Warna stabilo dan teks /
Highlight and text colour", "Pilih warnanya. / Pick the colours."; Image
"Gambar / Image", "Tambah gambar dari file, paling banyak 4. / Add an image
from a file, up to 4."; More "Lainnya / More", "Alat yang tidak muat di bar.
/ Tools that do not fit on the bar." The footer message (image refusal, the
delete's second click) stays where it is, drawn in the note.

Colour popover: the six paper swatches get their paper name (`Paper::label_key`).
Reminder popover: the six chevrons get "Hari sebelumnya / Previous day", "Hari
berikutnya / Next day", "Jam sebelumnya / Earlier hour", "Jam berikutnya /
Later hour", "Menit sebelumnya / Earlier minute", "Menit berikutnya / Later
minute", about "Roda mouse di kolom ini juga bisa. / The wheel over this
column works too."

### Sticky note list window (`widgets/list_win.rs`)

| Control | Name | About | Key |
|---|---|---|---|
| Plus | Catatan baru / New note | Catatan tempel baru di desktop. / A new sticky note on the desktop. | H:StickyNote |
| Close | Tutup / Close | — | Esc |
| Row marks (new small hit areas on each glyph) | Disematkan di atas / Kept on top; Digulung / Rolled up; Diselipkan ke tepi / Tucked to the edge; Ada pengingat / Has a reminder; Pengingat sudah lewat / Reminder passed (check what Clock means); Tumpukan, n lembar / Stack, n sheets; n dari m dicentang / n of m checked; n gambar / n images | — | — |

### Transcript window (`transcript_win/`)

| Control | Name | About | Key |
|---|---|---|---|
| Play / Pause | Putar / Play; Jeda / Pause | Dari posisi di bar. / From the position on the bar. | Spasi / Space |
| Seek bar | Posisi / Position | Klik atau geser untuk lompat. / Click or drag to jump. | ← → |
| Copy + prompt (short form) | — | Salin transkrip dengan perintah ringkasan, siap ditempel ke AI. / Copies the transcript with a summary prompt, ready to paste into an AI chat. | Ctrl+C if that is this button |
| Make note (short form) | — | Simpan transkrip ini ke halaman Catatan. / Saves this transcript to the Notes page. | — |
| Open .md (short form) | — | Buka file transkrip di aplikasi bawaan Windows. / Opens the transcript file in the Windows default app. | — |
| Redo (short form) | — | Transkrip ulang, bisa dengan model lain. / Transcribes again, with another model if you like. | — |
| Settings (short form) | — | Pengaturan transkrip: model, bahasa, otomatis. / Transcript settings: model, language, auto. (match the real section) | — |

### Live caption card (`caption_card/`)

| Control | Name | About | Key |
|---|---|---|---|
| Close | Tutup caption / Close captions | Caption langsung dimatikan. / Turns live captions off. | H:Captions |
| Pause / Resume | Jeda caption / Pause captions; Lanjutkan caption / Resume captions | Teks yang sudah ada tetap. / The text so far stays. | — |
| Copy (short form) | — | Salin semua teks sesi ini, bukan cuma yang terlihat. / Copies the whole session, not just what shows. | — |
| Language chip | Bahasa caption / Caption language | Klik untuk ganti: ID, EN, Auto. / Click to switch: ID, EN, Auto. | — |
| Grip | Ubah ukuran / Resize | Ctrl + roda mouse mengubah ukuran teks. / Ctrl + wheel changes the text size. | — |

### Lyrics card (`lyrics_card/`)

| Control | Name | About | Key |
|---|---|---|---|
| Close | Tutup kartu lirik / Close lyrics card | — | Esc, H:LyricsCard |
| Prev / Play-Pause / Next | Lagu sebelumnya / Previous track; Putar / Play; Jeda / Pause; Lagu berikutnya / Next track | — | — |
| Grip | Ubah ukuran / Resize | — | — |

### Closed notch

| Control | Name | About | Key |
|---|---|---|---|
| Notice ✕ (`NoticeClose`) | Tutup / Dismiss | Tanpa membuka apa pun. / Opens nothing. | — |
| REC level, mic (`RecMuteCompact`) | Bisukan mic / Mute mic; Nyalakan mic / Unmute mic | — | H:RecordMute |
| REC level, PC audio | Bisukan audio PC / Mute PC audio; Nyalakan audio PC / Unmute PC audio | — | — |

No tip on the busy-toast logo slot (hover already peeks the full toast) or on
the bodies of the compact views.

### Open panel

Page dock (`PanelPage`): the page name (`Key::Page*`) plus one line, and the
page's hotkey where one exists (Search, Clipboard, Devices, Arrange):

| Page | About (ID / EN) |
|---|---|
| Kalender | Jadwal dari kalender kamu. / Your calendar events. |
| Pomodoro | Timer fokus dan istirahat. / Focus and break timer. |
| Rekam | Rekam suara atau layar, lalu transkrip. / Record audio or the screen, then transcribe. |
| Pengingat | Pengingat dengan jam. / Reminders with a time. |
| Catatan | Catatan, bisa ditempel ke desktop. / Notes you can stick to the desktop. |
| Musik | Lagu yang diputar dan liriknya. / What is playing, with lyrics. |
| Mixer | Volume per aplikasi. / Volume per app. |
| Clipboard | Riwayat yang kamu salin. / What you copied. |
| Notifikasi | Riwayat notifikasi Windows. / Windows notifications you got. |
| Rak berkas | Tempat singgah file, juga ke HP. / A holding place for files, also to your phone. |
| Sistem | CPU, RAM, disk, dan bersih-bersih. / CPU, RAM, disk and clean-up. |
| Perangkat | Flashdisk dan disk luar, lepas dengan aman. / Flash drives and external disks, eject safely. |
| CLI | Terminal kecil di panel. / A small terminal in the panel. |
| Susun | Atur jendela dengan template. / Lay out windows with templates. |
| Pintasan | Tombol pintas pilihan kamu. / Your shortcut tiles. |

Check each line against the page; reword where the page does more or less.

| Control (`Hit`) | Name | About | Key |
|---|---|---|---|
| `OpenSettings` | Pengaturan / Settings | Buka jendela Pengaturan Vale. / Opens Vale's Settings. | — |
| `CalPrev` / `CalNext` | Bulan / Minggu / Hari sebelumnya, berikutnya (by view) / Previous, next month / week / day | — | — |
| `PomoStartPause` | Mulai / Start; Jeda / Pause; Lanjutkan / Resume | — | — |
| `PomoReset` | Ulang / Reset | Fase ini mulai lagi dari penuh. / This phase starts over. | — |
| `PomoSkip` | Lewati / Skip | Langsung ke fase berikutnya. / Goes straight to the next phase. | — |
| `NoteSticky` | existing `StickyOnDesktopTip` / `StickyOffDesktopTip` | Muncul sebagai catatan tempel di desktop. / Shows as a sticky note on the desktop. (off: Catatannya tetap di sini. / The note stays here.) | — |
| Music `Prev` / `Next` | Lagu sebelumnya / Previous track; Lagu berikutnya / Next track | Geser di notch juga bisa. / A swipe on the notch works too. | — |
| Music `PlayPause` | Putar / Play; Jeda / Pause | Klik tengah di notch juga bisa. / A middle click on the notch works too. | — |
| Music `LyricsCard` | Kartu lirik / Lyrics card (open: Tutup kartu lirik / Close lyrics card) | Lirik di jendela kecil yang melayang. / Lyrics in a small floating window. | H:LyricsCard |
| `MixerMute(i)` | Bisukan {app} / Mute {app}; Nyalakan {app} / Unmute {app} | — | — |
| `ClipPin` | Sematkan / Pin; Lepas sematan / Unpin | (say what pinning keeps, from the code) | — |
| `ClipPlain` | Salin teks polos / Copy as plain text | Tanpa format dan warna. / Without formatting. | — |
| `ClipDelete` | Hapus / Delete | Hapus dari riwayat. / Removes it from the history. | — |
| `NotifRemove` | Hapus / Remove | Hapus dari riwayat. / Removes it from the history. | — |
| `ShelfRemove` | Keluarkan dari rak / Take off the shelf | File aslinya tidak dihapus. / The file itself stays. | — |
| `PhoneBack` | Kembali / Back | — | Esc |
| `PhoneShowQr` | Tampilkan kode QR / Show the QR code | Pindai dengan HP untuk tersambung. / Scan it with your phone to connect. | — |
| `ArrangePlay` | Terapkan / Apply | Jendela kembali ke posisi yang disimpan. / Windows go back to the saved places. | — |
| `RecPause` | Jeda rekaman / Pause recording; Lanjutkan rekaman / Resume recording | — | — |
| `RecMute(src)` | as the REC notch level groups | — | H:RecordMute for the mic |
| `RecToggle`, idle (short form) | — | Mulai merekam audio sekarang. / Starts an audio recording now. (Layar tab: Pilih layar, jendela, atau area, lalu rekaman dimulai. / Pick a screen, window or area, then the recording starts.) | H:Record (audio tab), H:RecordScreen (screen tab) |
| `RecToggle`, recording (the stop square) | Hentikan rekaman / Stop recording | Rekamannya disimpan. / The recording is saved. | as idle |
| `RecPickFile` (header icon) | Transkrip file / Transcribe a file | Pilih rekaman atau video, lalu transkripnya terbuka. / Pick a recording or video, then its transcript opens. | — |
| `RecFolder` (header icon only, not "N lagi di folder") | Buka folder / Open folder | the tab's folder, in Explorer | — |
| `ScreenDiscard` (the trash; none once it asks) | Buang video / Discard video | Rekaman berhenti, videonya ke Recycle Bin. Ditanya dulu. / Stops and moves the video to the Recycle Bin. Asks first. | — |
| `DeviceOpen` | Buka di Explorer / Open in Explorer | — | — |
| `DeviceDetail` | Detail / Details | (say what the details show) | — |
| `CliExpand` | Perbesar / Enlarge; Perkecil / Shrink | — | — |
| `CliFolderStar` | Jadikan favorit / Star; Hapus dari favorit / Unstar | — | — |
| `SearchBack` | Kembali / Back | — | Esc |
| `CleanBack` | Kembali ke Sistem / Back to System | — | — |

The Rekam rows follow the redesign (direction B) merged from `main`. Its
labelled controls (tabs, source switches, device lines, quality row, caption
row, list rows and pills, "Simpan" / "Buang" of the question) have no tip.

### Settings window

| Control | Name | About | Key |
|---|---|---|---|
| `PageMove` / `TileMove` | Naikkan / Move up; Turunkan / Move down | — | — |
| `PageGrip` / `TileGrip` | Seret untuk mengubah urutan / Drag to reorder | — | — |
| `HotkeyReset` | Kembalikan bawaan / Reset to default | Jadi {combo}. / Becomes {combo}. (or "Tanpa tombol pintas. / No hotkey.") | — |
| `WidgetAdd` | Catatan tempel baru / New sticky note | — | H:StickyNote |
| `Repo` | Buka di browser / Open in browser | — | — |
| Surface, cat colour, accessory swatches, monitor map | their names | — | — |

Steppers (− / +) get no tip: the row label and value already say it. The
accent swatches keep their current hover (name in the row description).

### Editors (`ui/editor.rs`, `editor_popover.rs`)

| Control | Name | About | Key |
|---|---|---|---|
| Close ✕ | Tutup / Close | — | Esc |
| Date popover Prev / Next | Bulan sebelumnya / Previous month; Bulan berikutnya / Next month | — | PgUp / PgDn |

## What must not change

- What every button does, the hit areas, the hover plates and fades that
  exist (dock magnify, REC mute pill, notice ✕ disc, row hovers).
- The island's open / close hover logic (`HoverTracking`, dwell, linger).
- The capture overlays' own tips and the shot editor footer hint.

## Done means

- The shared tooltip and every row of the tables above, both languages.
- Unit tests for placement (below, flip above, clamp, notch, DPI scale), the
  delay / warm / clicked-until-leave rules (pure state machine, no window),
  text wrapping width, and that every tip key exists in both languages.
- `cargo test --locked` green; a live check on the real app: sticky note
  header, panel dock and a page, a card, Settings, light and dark theme, two
  monitors if available; idle CPU unchanged with nothing hovered.
- Memory note and README updated, merged to `main`, pushed.

## Calls made after the builder check (builder's, owner delegated)

- **Rekam page:** this branch does not wait for the Rekam redesign. It gives
  tips to the Rekam `Hit` variants on `main` when it merges `main` in. If the
  redesign lands first, its new icon buttons get tips here; if this lands
  first, the tooltip API is on `main` and the redesign (or a short follow-up)
  adds its rows. "Every row" means every row whose control exists at merge.
- **Sticky list row marks:** hover only. The mark rects are reported to the
  tooltip, a click anywhere on the row does what it does now.
- **Keyboard tips:** only for icon buttons that can already take focus
  today. No new focus rings or Tab stops in this change.
- **Fixed keys** (Ctrl+Shift+9, Esc, Space, arrows, PgUp / PgDn, right-click)
  are hand-written in the i18n keys, and each one is checked against its
  real key handler in review. A global hotkey with no combo shows no key
  plate (there is no separate "enabled" flag; no combo means off).
- **Theme tokens:** the plate uses the palette tokens above. If a pair fails
  4.5:1 in a theme, the builder may pick another token of the same palette
  (or a darker / lighter step) for that theme and writes the measured
  numbers in the memory note.
- **Z-order:** the tip is above every Vale window, pinned notes and cards
  included: `HWND_TOPMOST` without activation, asserted on every show.
- **Anchors:** the seek bar's tip anchors to a small rect at the pointer's x
  on the bar (it does not follow the pointer once shown); a grip anchors to
  the grip.

## Left to the builder

The module layout, the exact sizes within the ranges above, how each window
feeds hover to the module, and the wording fixes the code demands.
