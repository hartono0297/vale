---
tags: [decision, lyrics, window, hotkeys, media, power]
date: 2026-11-04
---

# The floating lyrics card

A separate always-on-top window that shows the synced lyrics of the session the
island shows, like the lyrics card of a music app. Built from an owner-approved
mockup; no questions could be asked while building, so the choices below that
the brief left open are listed under "Assumptions".

Related: [[lyrics-sync]], [[lyrics-lookup]], [[media-session-priority]],
[[hotkeys]], [[settings-structure]], [[bata-idle-power-budget]],
[[terminal-windows]], [[exe-size]], [[test-spawns-no-window]],
[[notch-monitor]].

## Where it lives

- `src/lyrics_card/` (declared in `main.rs`):
  - `layout.rs`: pure. Sizes, `card_rects`, `stack`, `scroll_target`,
    `scroll_bounds`, `hit_line`, `ManualScroll`, `seek_ms`, `progress`,
    `timer_ms`, `is_drag`, `resized`, `zone_at`, `card_next` (+ `Req`),
    `place` (+ `Mon`).
  - `colour.rs`: pure. `dominant_rgb`, `card_gradient`, `palette_gradient`,
    contrast maths.
  - `window.rs`: the Win32 window, the software Direct2D surface, DirectWrite
    text, WIC art, input. State is the module's own `thread_local! CARD`.
  - `mod.rs`: the public API (`open`, `update`, `close`, `release`, `hwnd`,
    `set_hidden`, `shown_key`, `take_placed`), `CardContent`, `OpenArgs`,
    `Msg`, `WM_APP_LYRICS_CARD` (**WM_APP + 70**; 41, 50-52, 60, 61 were taken).
  - `demo.rs` (debug builds): `--demo-lyrics-card`.
- `src/app_lyrics_card.rs`, a `#[path]` child of `app` (`mod card`):
  `LyricsCardState` (open for this run only, what was last sent, the lines key,
  an art fingerprint, hidden by fullscreen), `toggle_lyrics_card`,
  `close_lyrics_card`, `sync_lyrics_card(now)`, `on_lyrics_card_msg`,
  `check_lyrics_card_fullscreen`. `sync_lyrics_card` is called at the end of
  `refresh` and right after a lyrics answer arrives.
- The window procedure never calls `app::with_app`. It posts
  `PostMessageW(island, WM_APP_LYRICS_CARD, code, value)`; codes `Close` 1,
  `Seek` 2 (value = ms), `Prev` 3, `PlayPause` 4, `Next` 5, `Placed` 6 (the rect
  is in `take_placed()`). The `RefCell` borrow is never held across `ShowWindow`,
  `SetCapture`, `ReleaseCapture`, `DestroyWindow` (collected as `Fx` and run
  after); the procedure uses `try_borrow_mut` everywhere, so a message that
  arrives re-entrantly (for example `WM_DPICHANGED` during
  `UpdateLayeredWindow`) is skipped; `finish_move` re-reads
  `GetDpiForWindow` at the end of a drag, so a DPI change is never lost.

## Opening and closing

- The Musik page: a 26 x 26 round button (`Icon::Ext`) left of "Lirik", 8 dip
  gap, centred on the transport row (`ExpandedLayout::lyrics_card`,
  `media_panel_layout`). `Hit::LyricsCard` -> `Effect::ToggleLyricsCard`. The
  region and the paint (`paint_lyrics_card_button`, accent outline and tint
  while open, `Frame::lyrics_card_open`) exist only while lyrics are on
  (`panel_layout::media_hits_for` drops the region otherwise, with no media it
  still drops the transport as before). "Lirik" keeps its meaning.
- Hotkey row 6 "Buka lirik", default Ctrl+Alt+L ([[hotkeys]]).
- `card_next(open, lyrics_on, req)`: a toggle with lyrics on flips it; with
  lyrics off it does nothing and keeps it closed; `LyricsOff` and `Close`
  close. Lyrics off by any path (the Musik toggle `SaveLyrics(false)`, the
  Settings switch, `adopt_config`) closes it: `perform` closes at once and
  `sync_lyrics_card` (end of every `refresh`) closes whatever got past.
- Close: the X, Esc on `WM_KEYDOWN` while the card has focus, the hotkey or the
  button again. A click outside does NOT close it. Open state is for this run
  only, never reopened at startup. The notch click is untouched.
- Window: class `ValeLyricsCard`, `WS_POPUP`, `WS_EX_LAYERED | WS_EX_TOOLWINDOW |
  WS_EX_TOPMOST` (no taskbar button), activatable (a click takes focus so Esc
  works), opened with `SW_SHOWNOACTIVATE` so the hotkey never steals focus.
  Like the other Vale windows it is not in `capture::win::KEPT_OUT`.

## Content, status, lyrics reads

- The card follows the island's shown session (`state.media()` and
  `state.timeline()`); the ranking is untouched.
- New read-only accessors on `LyricsState`: `lines_for(q)`, `card_status(q)`
  (`CardStatus` Pending / Synced / Missing) and `wait_for` (the body of `wait_ms`
  extracted, `wait_ms` calls it; same numbers, pinned by a test). Two new
  fields, `none_key` (a definitive "none" for that key) and `gave_up` (retries
  used up), are written in `found` and cleared when that key is asked again.
  `want` and `found` return exactly what they did, no request changed.
- The cached lines already carry the intro shift (`pick_best` returns the
  shifted LRC, `format_lrc`), so the card does not shift again; a click seeks
  by the times the lines hold (test `a_shifted_lrc_seeks_by_the_times_it_holds`).
- Status mapping in `card_content`: no media -> "Nggak ada lagu yang diputer";
  media but no query (no title, artist or length) -> "none"; else `card_status`.
  Messages (`Key::LyricsCardSearching`, `LyricsCardNone`, `LyricsCardNoMedia`,
  both languages): "Mencari lirik..." with a centred note glyph while pending;
  "Lirik sinkron nggak ketemu buat lagu ini" for none, instrumental-only, plain
  only or gave up. Plain lyrics are never shown.
- The snapshot (`CardContent`) carries the art only when it changed (a 64-byte
  sampled fingerprint) and the lines only when the lyrics key changes; `update`
  is called only when the rest changed.
- The current line is chosen exactly as the notch does:
  `current_line(lines, lyric_position(position_at(timeline, playing, now)))`.

## Look

- 300 x 420 dip default, min 240 x 300, max 520 x 800, radius 16, a vertical
  two-stop gradient from the art. Header (14 dip padding): 30 dip thumbnail with
  a 6 dip radius, title (DM Sans 600, 13 px, white) over artist (12 px, white
  60 %), both one line with an ellipsis, a 24 dip close hit square with
  `Icon::X`. Body: lines wrapped with DirectWrite in the UI font
  (`Face::for_role_in(.., Role::Text, config.font)`), 17.5 px plain, drawn
  about 1.15x larger (20 px) and weight 600 when current, pure white; the others
  white 40 %; an empty LRC line is the note glyph; the body fades out over 28
  dip at its top and bottom (a layer with a vertical gradient opacity brush);
  the current line's centre rests at 40 % of the body. Each line's slot is the
  taller of its plain and its scaled bold form, so the bigger current line never
  covers its neighbour. Footer: a 3 dip bar (track white 20 %, fill the accent).
  Hover: prev / play-pause / next fade in above the bar, the grip (16 dip,
  bottom right) shows, and with `can_seek` the line under the pointer gets a
  white 8 % rounded highlight.
- Non-Latin text goes through DirectWrite's font fallback like the notch lyric
  (checked in the demo: Cyrillic, CJK and Hangul show, no tofu). The demo's
  non-Latin text is written as `\u{..}` code points (`ui/font_coverage.rs` scans
  every non-test source character).
- Track change: the gradient crossfades over 300 ms; each line change eases the
  list 350 ms (ease-out cubic). Reduced motion (`client_animations_on()` false,
  passed as `CardContent::animations`): no ease, no crossfade, no hover fade;
  values jump.

## Contrast (colour.rs, measured by a test that prints them)

Text is always white. `card_gradient` darkens the art colour in linear light
(hue kept) until both stops give white >= 7:1 and the dimmed lines (40 % white
composited in sRGB over the stop) >= 3:1; the top stop is about luminance
0.045 or less, the bottom about 0.012. The accent is a lighter tint of the art
colour with >= 3:1 against the bottom stop. `dominant_rgb` averages the 16 x 16
art pixels (straight BGRA), skips alpha < 32 and weights saturated, not-dark
pixels more (hue kept). Without art `palette_gradient` takes the dark window
palette's `card` and `bg` (`theme::window_for(accent, surface, Dark, false)`, so
the Terminal neutral family applies, [[terminal-windows]]) and the accent, kept
inside the same rules.

| art | top | L | cur | dim | bottom | L | cur | dim | accent vs bottom |
|---|---|---|---|---|---|---|---|---|---|
| white FFFFFF | 3C3C3C | .0452 | 11.03 | 3.20 | 1D1D1D | .0123 | 16.86 | 3.76 | FFFFFF 16.86 |
| yellow FFE600 | 443C00 | .0446 | 11.10 | 3.17 | 211D00 | .0120 | 16.93 | 3.76 | FFF173 14.58 |
| cyan 00E5FF | 00434B | .0452 | 11.03 | 3.07 | 002125 | .0122 | 16.88 | 3.73 | 73F1FF 12.66 |
| red E02020 | 650808 | .0296 | 13.19 | 3.06 | 410303 | .0120 | 16.95 | 3.48 | EE8484 6.67 |
| mid-grey 808080 | 3C3C3C | .0452 | 11.03 | 3.20 | 1D1D1D | .0123 | 16.86 | 3.76 | B9B9B9 8.59 |
| black 000000 | 000000 | 0 | 21.00 | 3.66 | 000000 | 0 | 21.00 | 3.66 | 737373 4.43 |
| blue 2020FF | 10109F | .0298 | 13.15 | 3.01 | 070768 | .0120 | 16.94 | 3.40 | 8484FF 5.43 |
| green 20D040 | 054610 | .0445 | 11.11 | 3.08 | 012305 | .0122 | 16.88 | 3.71 | 84E596 10.96 |

Saturated colours (red, blue) end up under the 0.045 luminance because their
dimmed composite is brighter than a grey's; the search keeps darkening until the
3:1 holds. Black art gives a pure black card.

## Seek maths

`seek_ms(lines, i) = lines[i].at_ms + OUTPUT_LAG_MS (100) + SEEK_MARGIN_MS (30)`,
capped at `lines[i+1].at_ms + OUTPUT_LAG_MS - 1` so it stays below the next
line's due time. Then `current_line(lines, lyric_position(seek_ms)) == i` for
distinct line times (two lines with one timestamp cannot be told apart: the last
wins). Tests cover the lag, lines 31 and 40 ms apart, a line at 0 and a shifted
LRC. `App` sends `MediaCommand::Seek(ms)` (`TryChangePlaybackPositionAsync(ms *
10_000)`, saturating) to the shown session only if `MediaInfo::can_seek` is
still true and the session key equals the one the card showed
(`lyrics_card::shown_key`).

`MediaInfo::can_seek` is `GetPlaybackInfo().Controls().IsPlaybackPositionEnabled()`
of the shown session, false on any error; the watcher reads it in
`Tracked::read_status` (the one place playback info is read for status), so
`choose` and the ranking are untouched. The other `MediaInfo` constructors say
`can_seek: false`. `media_log` prints `Seek(ms)` through the same `{:?}` (no
title, artist or other user data); card commands are not click-logged
(`media_click` is cleared before `Effect::Media`).

## Pointer and wheel

- Drag (header or body, not the X, the hover controls or the grip) and resize
  (the bottom-right grip) are implemented with `SetCapture` and
  `WM_MOUSEMOVE`, so `UpdateLayeredWindow` gets the new origin or size at every
  step. A press that moves more than `SM_CXDRAG` / `SM_CYDRAG` is a drag; a
  press and release without that is a click (on a line: a seek).
- Wheel (`WM_MOUSEWHEEL`): 48 dip a notch, clamped so the first or last line can
  reach the 40 % line; after 3 s without input (`MANUAL_HOLD_MS`, a one-shot
  timer, restarted by each notch) the list eases back to the song. The wheel is
  never forwarded: the card is a separate window, so the island's volume
  gesture never sees it.
- Leaving: `TrackMouseEvent(TME_LEAVE)` and `WM_MOUSELEAVE`.

## Placement

`config.lyrics_card` (`CardPlace` left, top, right, bottom in physical px, read
leniently like `settings_window`: a bad value is None, the rest of the file
loads; island-owned, in `merge_owned`). Saved at the end of a drag or resize (the
`Placed` message, through `save_config`). `layout::place`: a saved rect with at
least half of its area on one monitor is used (clamped to the size limits at
that monitor's DPI, top left kept); otherwise (first open, monitor gone) the
notch monitor's work area (`notch_monitor::resolve`): right edge 24 dip in from
the right, top 72 dip below the work area's top, the default size, shrunk to
fit a small work area. DPI: the monitor's at open, `WM_DPICHANGED` (suggested
top left, the same size in dip) and a re-read after a drag.

## Timers and power ([[bata-idle-power-budget]])

All on the card's own hwnd, in its own id space; nothing repeats and nothing
polls.

- `TIMER_LINE` (1): a one-shot at `min(next-line wait, 1 s)`
  (`lyrics::wait_for`, `layout::timer_ms`), re-armed by every snapshot and every
  fire; armed only while the card is visible (open and not hidden), the media is
  playing, the lyrics are synced and a timeline exists. Paused, no media, no
  lyrics or hidden: no timer at all (a test checks it with `KillTimer`).
- `TIMER_MANUAL` (2): a one-shot 3 s return, only while a wheel hold exists.
- Animations (the scroll ease, the line emphasis, the crossfade, the hover
  fade) run a `ui::frame_clock::FrameClock` on the card hwnd only while
  something moves and drop it once settled.
- Fullscreen: on the existing 1 s `TIMER_SYSTEM` tick, only while the card is
  open, `sources::fullscreen::is_fullscreen(Some(card_hwnd))` (the card's own
  monitor; own-process windows are skipped by the z-order walk, so the card does
  not count itself). Skipped while `arrange_overlay::active()`. Fullscreen
  hides it (`SW_HIDE`, timers and clock stopped) and it returns with
  `SW_SHOWNOACTIVATE`. The notch, the toasts and the panel are not affected.
- Exit: `release_graphics` calls `lyrics_card::release()` (destroys the window
  and drops the Direct2D objects) before `end_process` terminates, so no WARP
  object survives to process detach (the root cause in `end_process`).
- Closing the card drops its surface (factory, target, brushes, `IconSet`), so
  nothing of it stays while closed.

## Demo (`--demo-lyrics-card [scene]`, debug builds)

Scenes: `synced` (default), `cyrillic`, `cjk`, `loading`, `none`, `nomedia`,
`noart`, `long`. A fake playing timeline loops through the lines (a one-second
demo-only timer); the art is a red 32 x 32 BMP built in memory. `DI_LANG=en`
works; `DI_CARD_POS=<ms>` freezes the position. Seek, prev, play/pause and next
are printed, never sent (a seek only moves the fake timeline). The place goes
to `temp_dir()/vale-lyrics-card-demo.json` and is read back at the next start,
never the real config. No media session, media key or user data is touched.
Checked by eye with screenshots of the window (all eight scenes, hover, wheel
and its return, a click on a line, a drag, a resize, the X and Esc) and of the
Musik page (`DI_DEMO_STEP=media-lyrics-on`) and `--settings hotkeys`.

## Tests

`layout.rs` (stacking, the 40 % target, before the first line, last line,
wrapped heights, hit testing with the offset, manual hold and 3 s return, seek
with the lag, a shifted LRC and the cap, placement and its fallback, toggle
rules, sizes and zones), `colour.rs` (dominant colour, gradient contrast for
eight kinds of art, hue kept), `window.rs` (open, update, hide, close and the
timers without a window on screen: `ON_SCREEN = !cfg!(test)`, [[test-spawns-no-window]]),
`lyrics.rs` (`card_status`, `lines_for`, `wait_for` equal to `wait_ms`, no request
changes), `config.rs` (the rect round trip, lenient read, an old config, the
hotkey row's default, None, garbage, a duplicate of an earlier row, migration, an
old file without the key), `hotkey.rs`, `app_hotkeys.rs` and `app_prtsc.rs` at
six rows, `panel_layout.rs` (the button between `next` and "Lirik" at every
tested size, the region only with lyrics on), `settings_layout.rs` (six rows, no
viewport), `state.rs`, `media_log.rs`.

## Assumptions (the brief left these open)

- The card is left aligned (not centred) in the body.
- A paused track with no lyrics asked yet shows "Mencari lirik...": the lookup
  runs only while playing (`want(.., fetch = playing)`, which must not change),
  so the card says "searching" until the track plays.
- Media with no query (no length yet) is shown as "none", not "searching".
- The current line switches to the bold face halfway through its 350 ms ease
  (a weight cannot be tweened); alpha and size ease smoothly.
- Black art gives a black card; its accent is a mid grey tint.
- The thumbnail is the art scaled to a square (not cropped).
- A seek position is passed as the timeline's own position (the session's
  `StartTime` is assumed 0, as `read_timeline` subtracts it for reads only).
- The `Placed` message that arrives while `App` is borrowed is dropped (the card
  keeps the rect until the next one).
- The Settings "Tombol pintas" description reads "Kartu lirik melayang" /
  "Floating lyrics card".

## Not verified without real media

Real YouTube Music / Spotify / browser sessions: `IsPlaybackPositionEnabled`
values per player, `TryChangePlaybackPositionAsync` actually seeking (and the
position the player lands on for a click), lyric timing against audio, the
play/pause, prev and next buttons reaching the right session, the fullscreen
hide with a real game or video on the card's monitor (and on a second monitor),
a DPI change by dragging across monitors of different scale, the hotkey
Ctrl+Alt+L against other apps, and reduced motion with the Windows animations
setting off (only the code path and the tests cover it). Esc needs the card to
have focus (a click gives it; the hotkey opens it without focus on purpose).

## Exe size ([[exe-size]])

`cargo build --release --locked` of `main` (v3.17.0, 3aa7fa7, from `git archive`)
and of the branch, each with its own `CARGO_TARGET_DIR`: 3,186,688 and
3,233,280 bytes, **+46,592 bytes (+1.46 %)**. The two source folders do not have
names of the same length (the embedded PDB path moves an exe by up to 512
bytes). The added code is the card (window, layout, colour, WIC decode), the
sixth hotkey row, the Musik button and the seek command; no new crate and no new
`windows` feature.
