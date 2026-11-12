# Rekam page redesign (direction B)

Date: 2026-11-12. Owner's request: "ubah UI dan UX design modul ini, kasih gw visualnya dulu".
The owner skipped the questions, compared three directions (`docs/design/rekam-ulang.html`)
and picked **B**, then approved every state drawn in `docs/design/rekam-b.html` ("Udah pas").
The mockup is the visual source of truth; this document says what it means in code.

Read first: `docs/memory/recording-audio.md`, `recording-controls.md`, `screen-recording.md`,
`recording-quality.md`, `transcript.md`, `live-captions.md`, `menu-pop.md`, `panel-pages.md`.

## Why

The page works but reads as three loose groups (a lone Start card, a source segment with
device lines, a caption switch on its own row), a header with four controls of four shapes,
list rows that repeat the date ("Recording 8 Nov 01:08" over "8 Nov · 6 s"), transcript pills
of different widths and fills, and a list that stops at four rows with no hint there are more.
Direction B splits the page into the tool (left card) and the results (right list).

## Goals

- Left: one card holding everything that decides a recording (button, sources with their
  devices, live captions). Right: the list, the full height of the page.
- Every row action has one shape; list rows grouped by day so a row needs only time and length.
- Header: tabs on the left, two icon buttons (transcribe a file, open folder) and the Settings
  button on the right.
- Every state in the mockup, in both languages, on every island surface (Terminal included)
  and both UI fonts.

## Non-goals

- No keyboard focus on the panel (it has none today; the global hotkeys stay the keyboard path).
- No change to the closed REC notch, the caption card, Settings, the recorder, the transcript
  engine, the folder layout (the `rec-one-folder` session owns that), or the hotkeys.
- No scrolling list on the page; older recordings stay in the folder.
- Tooltips: the `hover-tooltips` session adds the tooltip mechanism. This change gives the two
  header icon buttons their tooltip text through whatever that session lands; if it has not
  landed when this merges, the icons ship without tooltips and that is reported, not built here.

## Anti-goals (a failure even if it works)

- A wider or taller panel. It stays 560 × 290 (`ShapeKind::Panel`).
- A new timer, thread or poll at idle (`bata-idle-power-budget`). The meters stay recording-only.
- Red or ochre used for anything but record/stop/discard and paused.

## Layout (dip, panel content box x 20..540, y 14..267)

- **Header row** (y 14, h 22): Audio / Layar track at the left (unchanged look; locked at 0.5
  alpha while recording). At the right, left of the Settings button: two 22 × 22 icon buttons,
  `filetx` ("Transkrip file lain…" / "Transcribe a file…", `Hit::RecPickFile`) then `folder`
  ("Buka folder" / "Open folder", `Hit::RecFolder`). The text links and their short forms go.
- **Body** (y 44, h 222): left card 196 wide (raised fill, radius 18, padding 10 × 12), 12 gap,
  right list (312 wide).

### Left card, idle (Audio tab)

- Top block 92 high, centred: the round record button (56, ring in `track`, red dot 22) and
  the label "Rekam" / "Record" under it. `Hit::RecToggle`.
- Two source rows, 34 high each, a `track` hairline above each:
  icon (mic / speaker; accent when on, crossed and secondary when off), name ("Mic", "Suara PC" /
  "PC audio") over the device line (10 dip, secondary, ellipsis, small chevron), switch at the
  right.
  - The switch toggles that source (`Hit::RecSources` with the new pair, written to
    `recording.sources`). The last source on cannot be switched off: its switch draws at 0.45
    alpha and takes no click.
  - The device line opens the existing device dropdown (`Hit::RecDevice(source)`, `menu_pop`).
  - A chosen device that is missing: the device line reads "Mic pilihan tidak ada" / "Chosen mic
    not found" (and the PC audio equivalent, existing keys) in ochre.
  - An off source still shows its device line (dimmed with the row).
- Caption row, 34 high: CC icon, "Caption langsung" / "Live captions", switch
  (`Hit::RecCaptions`, unchanged behaviour). Same on both tabs.

### Left card, recording (audio and screen)

- Top block: pause/resume (round 32, `Hit::RecPause`) and stop (ring 52 with a red square,
  `Hit::RecToggle`) side by side, centred; for a screen recording a round 32 discard button with
  a trash icon follows stop (`Hit::ScreenDiscard`). Under them a red dot (ochre when paused) and
  the clock (15 dip, tabular; ochre when paused).
- Discard armed (first click): the top block shows "Buang video ini?" / "Throw this video away?"
  over two 26-high buttons: "Buang" / "Discard" (10 % red tint, soft red text, `ScreenDiscard`
  again) and "Simpan" / "Keep" (track fill, disarms). The arm timeout stays whatever it is today.
- Source rows: a 24 mute button replaces the icon (`Hit::RecMute`; muted = outline and crossed
  icon), the device line stays clickable, a 5-bar meter replaces the switch. A muted source's
  meter rests and its device line reads "Dibisukan · Ctrl+Alt+M" / "Muted · Ctrl+Alt+M" (the
  mic mute hotkey text from `config.hotkeys`, so it follows a rebind; PC audio has no hotkey and
  reads "Dibisukan" / "Muted"). Paused: meters rest.
- Caption row unchanged.

### Left card, idle Layar tab

- Top block as audio. Under it a hint, 10.5 dip secondary centred: "Layar, jendela atau area:
  dipilih setelah tekan Rekam" / "Screen, window or area: you pick it after Record".
- A row "Kualitas" / "Quality" with the current "1080p · 30 fps" (from `video_quality` and
  `video_fps`, `Asli` / "Original" when uncapped) and a chevron: opens a `menu_pop` with two
  groups, the four qualities then 30 / 60 fps, checked as set. Picking writes the config like
  Settings does (same refusal while busy, though the row only shows idle).
- Caption row.

### Right list

- Rows grouped by day with a 16-high group label (10.5 dip secondary): "Hari ini" / "Today",
  "Kemarin" / "Yesterday", else "8 Nov". Rows 28 high: 22 well with the kind icon (mic, or
  monitor for video), the start time (12.5 dip, 500), the screen recording's title when it has
  one (ellipsis), the length (secondary; "belum selesai" / "unfinished" for a `.part`), then the
  transcript button at the right. A row click opens the recording as today (`Hit::RecOpen`).
- Transcript button: 22 high, min 76 wide (grows to fit "Menunggu model"), radius 8, 11 dip 500.
  - Transkrip / Transcribe: track fill. Lihat / View: no fill change, 1 dip accent inner border.
  - n%: raised fill with a track border and a `raised_hi` fill from the left to n %.
  - Antre / Queued and Menunggu model / Waiting for model: transparent, track border, secondary text.
  - Gagal / Failed: track fill, soft red text (`SOFT_RED`).
  - Off (transcripts disabled): Transkrip at 0.45 alpha, as today. Silent video: no button.
  - Hover: `raised_hi` fill.
- When more recordings exist than rows fit: a last line "N lagi di folder" / "N more in the
  folder" with a chevron, secondary, `Hit::RecFolder`. The scan reports the total count.
- Recording: a "Sekarang" / "Now" row first under today's label, with a red dot (ochre paused)
  in the well and "Merekam" / "Recording" (or "Dijeda" / "Paused"); it takes no click.
- Empty: a 32 well with the kind icon, "Belum ada rekaman" / "No recordings yet" and "Tekan
  Rekam, atau Ctrl+Alt+R dari mana aja" / "Press Record, or Ctrl+Alt+R from anywhere" (Layar:
  the screen hotkey). Hotkey text from `config.hotkeys`.

## Colours

Island palette only: `raised` card, `raised_hi`, `track`, `text`, `secondary`, `accent` (switch
on, source icon on, View border, active dot), `REC_RED` (record dot, stop square, live dot),
`ochre` (paused, missing device), `SOFT_RED` (Failed, Discard). Checked in the mockup:
secondary on raised 6.06:1 (Terminal 6.47), on raised_hi 4.74 (Terminal 4.69); soft red on
track 6.11 (Terminal 5.20); ochre on raised 7.39; soft red on the discard tint ≥ 5.4.

## Copy

New keys (Indonesian, English): "Rekam" / "Record" (button label; replaces "Mulai"), "Sekarang"
/ "Now", "Kemarin" / "Yesterday" if not present, "{n} lagi di folder" / "{n} more in the
folder", "Dibisukan" / "Muted", "Buang video ini?" / "Throw this video away?", "Simpan" /
"Keep", "Kualitas" / "Quality", the Layar hint, the two empty hints. Every string through
`i18n.rs`, no em dash.

## Calls made after the builder check (the session's, not the owner's)

- **Waiting strip.** When the strip shows, the content box is 24 dip shorter. The card and the
  list both follow the available height: the card's top block drops the "Rekam" label and 8 dip
  of padding (92 to 68; while recording the clock stays and the gap shrinks), the source and
  caption rows keep their 34. The list fits rows by remaining height. Nothing runs over the dots.
- **Folder work in parallel.** `rec-one-folder` (plan stage, not on `main` yet) keeps the per-tab
  folder button (Audio opens the audio folder, Layar the video folder) and only changes the
  paths. Plan against today's per-tab dirs. Both touch `scan_recent`; whichever merges second
  merges `main` first and keeps both sides.
- **"More" line.** Rows are cut by remaining height (as `rows_that_fit` does), reserving the
  more line when not all fit; group labels count against the height. `N` counts the tab
  folder's listed recordings (`.part` included, as the list shows them) minus the rows shown;
  the recording in progress is never counted. The scan returns the first `limit` entries plus
  the total.
- **Unbound hotkeys.** No mute hotkey: "Dibisukan" / "Muted" alone. No record hotkey: the empty
  hint is just "Tekan Rekam" / "Press Record".
- **Retired strings.** "Buang video ini?" replaces "Yakin buang?" on the page; "Rekam" replaces
  "Mulai" as the button label. A key is removed only when nothing else uses it (check the tray
  and the notch). The closed notch and its code stay untouched.
- **Icon button hover.** The two header icon buttons get their own hover fill (`raised`, icon in
  `text`), with or without the tooltip text.

## What is left to the builder

Exact paint order, hit-region rects, how the scan returns the total and the start date for
grouping, test names, and the demo scenes for each mockup state (extend `--demo-record`).

## Done means

- Every state in `docs/design/rekam-b.html` renders in the app (demo scenes), both languages,
  Espresso and Terminal, DM Sans and JetBrains Mono, with nothing clipped.
- Layout tests: no overlap, everything inside the content box, the last source cannot be turned
  off, the row button width rule, the "more" line only when rows are cut.
- `cargo test --locked` green on `main`.
