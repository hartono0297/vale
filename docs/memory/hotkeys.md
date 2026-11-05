---
tags: [decision, hotkeys, settings, config, windows]
date: 2026-11-04
---

# Hotkeys: eight recordable combos in one Settings section

The five fixed presets (Clipboard Ctrl+Alt+V / Win+Alt+V, search
Ctrl+Alt+Space / Win+Alt+S, Devices Ctrl+Alt+D, Arrange Ctrl+Alt+S, the PrtSc
toggle) became **recordable combos**, one row each in Settings → **Tombol pintas**
(Hotkeys), the ninth section right after Clipboard ([[settings-structure]]).
Related: [[search]], [[clipboard-history]], [[devices]], [[arrange]],
[[screenshot-snap]].

**Sixth row (2026-11-04): "Buka lirik" / "Open lyrics"** opens the floating
lyrics card ([[lyrics-card]]). `HotkeyId::LyricsCard`, appended LAST in
`HotkeyId::ALL` (so the order and the ids of the first five never change),
config key `lyrics_card` inside `config.hotkeys`, **default Ctrl+Alt+L**, may be
None, `RegisterHotKey` id `HOTKEY_LYRICS_CARD` = 6, labels `HotkeyRowLyricsCard`
("Buka lirik") and `HotkeyRowLyricsCardDesc` ("Kartu lirik melayang" /
"Floating lyrics card"). The press does nothing while lyrics are off. Every
table sized by the row count is `HotkeyId::ALL.len()` now (`ROWS` in
`app_hotkeys.rs`, the PrtSc table, the Settings failed flags). Everything below
that says "five" is five rows plus this one; the rest is unchanged.

**Eighth row (2026-11-05): "Daftar catatan tempel" / "Sticky note list"** opens
the sticky note list window ([[sticky-note-list]]). `HotkeyId::StickyList`,
appended after `StickyNote` (index 7), config key `sticky_list`, **no default**,
`RegisterHotKey` id `HOTKEY_STICKY_LIST` = 8, labels `Key::StickyList` and
`Key::StickyListDesc` ("Cari dan buka catatan tempel" / "Search and open sticky
notes"). The press calls `open_note_list(None)`. A config from before the row
has no `sticky_list` key and reads None (tested). The ids are 1-8 (6 lyrics card,
7 new sticky note, 8 the list) and Settings -> Tombol pintas has **eight rows**
and still scrolls at the minimum size.

## Model (`src/hotkey.rs`, pure)

- `Mods { ctrl, alt, shift, win }`, `Hotkey { mods, vk }`, `HotkeyId`
  (`ALL` = Search, Clipboard, Devices, Arrange, Screenshot: also the row
  order), `Hotkeys` with `get`/`set`/`reset`/`reset_all`. Defaults: Search
  Ctrl+Alt+Space, Screenshot bare PrtSc, the rest None.
- **String form** (config.json and tests): modifiers in the order Win, Ctrl,
  Alt, Shift joined with `+`, then the key: "Ctrl+Alt+V", "Win+Shift+F13",
  "PrtSc". Off is "None". Parsing is case-insensitive, any modifier order,
  blanks ignored; aliases Control, Windows, PrintScreen / PrtScn, Del, Escape;
  "", "none", "off" and JSON null are None. Key names: A-Z, 0-9, F1-F24, Space,
  PrtSc, Pause (VK_CANCEL 0x03, which Ctrl+Pause produces, is Pause),
  ScrollLock, Insert, Delete, Home, End, PageUp, PageDown, the arrows, Enter,
  Tab, Esc, Backspace, Num0-Num9, NumMultiply/Add/Subtract/Decimal/Divide, the
  OEM names (Semicolon ... Quote), CapsLock, NumLock; any other vk is
  "VK_" + two uppercase hex digits and parses back. Display and parse
  round-trip; `caps()` gives the parts for key caps.
- A modifier vk (Shift, Ctrl, Alt, both sides; both Win keys) is never a
  combo's key; left and right are one flag.
- `check(hk)`: `Reject::Reserved` for a modifier vk (so "Win alone") and
  Alt+Tab, Alt+Shift+Tab, Alt+F4, Alt+Esc, Alt+Space, Ctrl+Esc,
  Ctrl+Alt+Delete, Ctrl+Shift+Esc, Win+L, Ctrl+Shift+V (paste as plain text in
  browsers and editors); `Reject::NeedsModifier` without Ctrl, Alt or Win
  (Shift alone does not count), except PrtSc, Pause, ScrollLock and F13-F24.
  `used_by(&Hotkeys, id, hk)` finds another row with exactly that combo.
- `register_mods(hk)` is the `RegisterHotKey` bits plus `MOD_NOREPEAT`
  (checked against the windows crate in a test); `uses_hook(hk)` is "vk is
  PrtSc".
- `record_step(vk, mods)` is the recorder's one decision, shared by the hook
  and the window fallback: a modifier key is `Ignore`, bare Esc `Cancel`, bare
  Backspace or Delete `Clear`, everything else `Combo`. Esc, Backspace and
  Delete with modifiers are ordinary combos and go through `check`.

## Config

- `config.hotkeys` serializes as an object of seven strings (five until 2026-11-04). It is read
  **leniently and never fails**: a missing key, a value that does not parse or
  fails `check` gives that row's default; anything that is not an object gives
  every default; a row that repeats an earlier row (in `ALL` order) reads as
  None. The rest of the config always loads. Island-owned (`merge_owned`), so
  a Settings choice sticks; the file is not re-read for it between saves.
- **Migration** (`try_parse` -> `migrate_hotkeys`, only when the file has no
  `hotkeys` key; `hotkey::from_legacy`): old names match in any case and a
  typo is off. Clipboard from `clipboard.hotkey` (CtrlAltV, WinAltV, else
  None). Search: if `quick_search_hotkey` is there, CtrlAltSpace / WinAltS /
  else None; if not, the 3.2.0 `search_hotkey` string that is not exactly
  "Off" (same mapping, a typo is None); otherwise Ctrl+Alt+Space. Devices from
  `devices_hotkey` (CtrlAltD), Arrange from `arrange.hotkey` (CtrlAltS),
  Screenshot from `screenshot.print_screen` (false: None; true, absent or a
  non-bool: PrtSc). The old keys, their enums (`ClipHotkey`, `SearchHotkey`,
  `ArrangeHotkey`, `DevicesHotkey`) and `ScreenshotSettings::print_screen` are
  gone; the old keys drop out on the next save.

## Registration (`src/app_hotkeys.rs`, child of `app`)

- Ids: HOTKEY_CLIP 1, SEARCH 2, DEVICES 3, ARRANGE 4, SCREENSHOT 5, LYRICS_CARD 6.
- `HotkeyState` keeps what each row has in effect, a `failed` flag per row, the
  hook thread, `paused` and `started`. `sync_hotkeys()` (startup,
  `adopt_config`, after a Settings change) plans each row from want, in
  effect and failed (`plan_row`): `RegisterHotKey` for an ordinary combo, an
  entry in the PrtSc hook's table for a PrtSc combo, `UnregisterHotKey` for
  off. **Two passes** (`HotkeyState::apply` over the `HotkeyOps` trait, a fake
  in tests): every changing row lets go first, then the new combos go in.
  `RegisterHotKey` refuses a combo still held by ANY id, the window's own
  other ids included, so one pass in `ALL` order failed when two rows swapped
  combos (e.g. "Kembalikan semua" moving Ctrl+Alt+Space from Clipboard back to
  Search). Then it starts the hook thread if any row needs it, stops it when none
  does. A hook install failure marks every PrtSc row failed.
- `retry_hotkeys()` retries failed rows when the tray menu or Settings opens.
  `WM_DESTROY` (and `WM_ENDSESSION`) unregisters every row and stops the hook.
- `WM_HOTKEY` id -> `on_hotkey(row)`; `WM_APP_PRTSC` (WM_APP+60) carries the
  row index in `wparam`. Firing behaviour (the hold in `state.rs`,
  `TIMER_CLIP_HOLD`, focus handling) is unchanged.
- Per-module `hotkey` / `hotkey_failed` fields and `sync_*_hotkey` are gone;
  `ClipState` got a plain `started` flag where `clip.hotkey.is_none()` used to
  mean "not started".
- **The PrtSc hook** (`app_prtsc.rs`) reads a static table of one `AtomicU8` per row (seven)
  (0xFF unassigned, else the row's modifier bits). `prtsc_hook_action(vk,
  key_down, mods, swallowing, table)` swallows and fires a press whose
  modifiers EXACTLY equal an assigned row's, swallows its auto-repeat and
  key-up, and passes everything else. Updating a row while it runs is an
  atomic store, no restart; the thread `vale-prtsc-hook` exists only while a
  row uses PrtSc and the hotkeys are not paused. Details of the hook itself:
  [[screenshot-snap]].

## Pausing while Settings records

`Action::HotkeyRecording(true)` -> `pause_hotkeys()`: unregisters every row's id,
empties the table, stops the hook, sets `paused`, marks nothing failed; while
paused `sync_hotkeys` does nothing. `HotkeyRecording(false)` (and
`Action::Closed`, as a safety) -> `resume_hotkeys()` clears `paused` and runs a
full sync. `Action::Hotkey(id, value)` re-checks `check` and `used_by`
defensively, sets, saves, syncs unless paused; `HotkeysResetAll` resets all.
The window posts `Hotkey` (if any) BEFORE `HotkeyRecording(false)`, and
`HotkeyRecording(false)` **waits for the recorder's hook thread to end**
(`WM_APP_HOTKEY_REC_DONE`, WM_APP+14, posted after the unhook; the window's
pure `Resume` decides). Resuming while the recorder still drains would put the
PrtSc hook back in, newer and so first in the LL chain: a held PrtSc's
auto-repeat (swallowed keys are up for Windows, so its modifiers read as none
too) would fire a capture right after recording. Only a key held past the
2 s drain limit can still do that.

## The recorder (`src/ui/hotkey_rec.rs`, `settings.rs`)

- Clicking a row's field starts recording that row (clears `hotkey_error`,
  posts `HotkeyRecording(true)`, starts the hook); clicking it again cancels.
  The field shows "Tekan tombol… (Esc batal)" under an accent border.
- **The hook** is a `WH_KEYBOARD_LL` on its own thread `vale-hotkey-rec`, built
  like `app_prtsc`'s (install, report over a channel, `GetMessageW` loop, stop
  with `WM_QUIT` and join). It posts `WM_APP_HOTKEY_REC` (WM_APP+13, unique
  among the Settings window's ids, checked in a test) with the vk and the
  modifier bits.
- It **swallows every key-down**, so Win combos, PrtSc, Alt+Tab and the like
  reach neither Windows nor another app. It **tracks the modifiers itself**
  (`GetAsyncKeyState` does not see swallowed keys): a 256-bit set (four
  `AtomicU64`) of the keys whose key-down it swallowed (`KeySet`, pure
  `RecState::event`). Auto-repeat is ignored. On the first non-`Ignore`
  `record_step` it posts once.
- Then it **drains**: only the key-ups of keys in the set (and their repeats)
  are still swallowed, everything else passes; a key-up of a key it never saw
  go down passes (swallowing it would leave that key held). With the set empty
  it posts `WM_QUIT` to itself, unhooks and the thread ends: the hook is never
  left installed.
- **Safety timers** (thread-level `SetTimer`): draining ends after 2 s at most;
  a recording with no combo in 20 s posts a cancel (vk 0) and drains.
- The proc allocates nothing, takes no lock, does not log.
- The window keeps the finished hook's handle only so `WM_DESTROY` can stop it
  at once (`RecHook::stop` posts `WM_QUIT` only while the thread's id is
  non-zero, so it never posts to a reused thread id).
- **Fallback**: if the hook cannot be installed, the window records from
  `WM_KEYDOWN` / `WM_SYSKEYDOWN` (modifiers from `GetKeyState`); Win and PrtSc
  are then not capturable, and Esc cancels instead of closing the window.
- On a combo: Cancel stops; Clear posts `Hotkey(id, None)`; Combo runs `check`
  then `used_by` against the window's model: reserved ("Dipakai Windows"),
  needs a modifier ("Tambahkan Ctrl, Alt, atau Win"), conflict ("Sudah dipakai
  {row label}", `i18n::hotkey_used_by`) or posts `Hotkey(id, Some(hk))`. The
  refusal shows red in that row's description until the next recording or a
  section change. A row set and taken by another app is red too ("Sudah dipakai
  aplikasi lain").
- A recording is cancelled (old value kept) by Esc, a press anywhere else (the
  click still does its normal thing), a section switch and `WM_ACTIVATE` with
  `WA_INACTIVE` (so Win+L ends it too).
- Reset buttons: per row (disabled at the row's default; refused in red when
  the default is another row's) and "Kembalikan semua".

## Layout and icon

`Icon::Keyboard` (71 icons now). `hotkeys_page`: one card of `HEADER_H` + 6 x
`ROW_H`, a hint and "Kembalikan semua" in the header like a list's, then per
row the name and description, a `Control::HotkeyField` (key caps joined by
"+", "Tidak ada" when off; width measured from its content, min 150, max 260,
and at the 720 px minimum less, `hotkey_max_w`, so the description keeps about
210 px; caps that do not fit fall back to plain text) and an `Icon::Reset`
button. The section never needs a viewport. Removed rows: Umum's search row
(and `UMUM_TRIM_*`), Clipboard's hotkey and PrtSc rows, Fitur's Perangkat and
Susun hotkey rows.

## Tests

Parse/format round trips, `check`, `used_by`, every legacy case, lenient read,
resets, `merge_owned`, the PrtSc table decision, `record_step`, the recorder
bookkeeping and drain, the `RegisterHotKey` bits, the message ids, both hook
threads starting and stopping with no window, the layout.

## Not verified live

Everything that needs real keys: the recorder hook swallowing Win combos and
PrtSc on real hardware (and that no key is left stuck afterwards), `RegisterHotKey`
conflicts with other apps, the fallback path, Win+L and Ctrl+Alt+Del (a hook
cannot catch them: they lock or open the secure screen, which deactivates the
window and cancels the recording), a hook under an elevated window (UIPI). The
Settings page was only looked at in the debug demo (`--settings hotkeys`,
`DI_HOTKEYS=failed` marks every set row as taken). Hand-edited combos that are
reserved, lack a modifier or repeat another row are read leniently as above.

## The seventh row: Catatan tempel baru (2026-11-04)

`HotkeyId::StickyNote` is appended to `ALL` after the lyrics card (seven rows, index 6, config key
`sticky_note`, default None, a config without the key reads None).
`RegisterHotKey` id 7 (`HOTKEY_STICKY`); id 6 is the lyrics card's (both are on
main now). The per-row arrays follow `HotkeyId::ALL.len()` / `prtsc::ROWS`.
A press makes one empty sticky note with the keyboard, or at 20 notes shows a
short island notice ([[widgets]]).
