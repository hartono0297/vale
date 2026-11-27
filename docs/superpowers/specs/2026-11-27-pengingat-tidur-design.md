# Pengingat tidur (sleep reminder): design

Date: 2026-11-27. Owner request: "tambahkan reminder untuk tidur di jam 01:00,
02:00, 03:00, 04:00 untuk tidur (seperti sapaan maskotnya)". Mockup the owner
saw: `docs/design/maskot-pengingat-tidur.html`.

## Intent

The owner stays up late at the PC. At 01:00, 02:00, 03:00 and 04:00 the chosen
mascot (cat, falk, octopus) shows up in the notch, the way the greeting does,
and nudges them to go to sleep. It gets sleepier each hour. What matters most to
the owner: "yang penting setiap 1 jam dari jam 1 pagi, ada pengingatnya", so a
reminder must not silently go missing while they are at the PC.

## Goals

- One notice per hour at 01, 02, 03, 04 local time while the owner is at the PC.
- The mascot's pose escalates by hour (decided, option A in the mockup):
  - 01: drowsy, half-closed eyes, slow blinks
  - 02: yawns once (mouth or beak opens wide, then closes)
  - 03: nods off (head sinks, eyes close, jerks back up)
  - 04: asleep with z's (today's `Expr::Sleep`)
- Text, fixed and bilingual (owner saw and kept these):

  | Hour | ID title | ID body | EN title | EN body |
  |---|---|---|---|---|
  | 01 | Sudah jam 1 | Yuk mulai siap-siap tidur | It's 1 AM | Time to start winding down |
  | 02 | Jam 2, masih melek? | Tidur yuk, besok lanjut lagi | 2 AM, still up? | Let's sleep, pick it up tomorrow |
  | 03 | Jam 3 pagi | Aku sudah ngantuk berat. Tidur yuk | 3 AM | I can't keep my eyes open. Bed? |
  | 04 | Sebentar lagi subuh | Tidur dulu, walau sebentar | Almost dawn | Get some sleep, even a little |

- Shows 6 s, then the notch goes back to what it showed (music view included).
- Settings → Maskot: a new toggle row "Pengingat tidur" in the Sapaan card,
  under the Sapaan row, on by default. Desc: "Jam 01.00 sampai 04.00, maskot
  ngajak tidur" / "1 to 4 AM, the mascot nudges you to bed". Disabled (dimmed,
  no click) while the mascot is off, like Sapaan.
- With the reminder on, the hour mascot story (`mascot_hours`, "Tiap jam")
  skips hours 1, 2, 3, 4, so the two never meet. Hours 0 and 5+ unchanged.

## Non-goals

- No "Cukup malam ini" / snooze chip (owner: "Tidak perlu"). A click on the
  notice or its ✕ closes that one hour's reminder only.
- No choice of hours, no custom text, no sound, no Windows toast.
- No catch-up after the hour's window (see Rules).
- The Night greeting "Masih begadang?" is unchanged.

## Anti-goals

- A timer that runs at idle between hours ([[bata-idle-power-budget]]): only a
  one-shot armed for the next reminder hour, plus 30 s retries inside a window.
- A reminder that depends on "Tiap jam", the story preset, motion Full or
  Energy Saver. Those only change whether the pose moves.
- A pose that leaves the notice badge or the notch (clamp tests must hold).

## Approaches

1. **Own one-shot timer (chosen).** `TIMER_SLEEP_REMINDER` armed for the next
   of 01/02/03/04:00 + 1 s, only while the reminder and the mascot are on. On
   fire, gates are checked, retried every 30 s within a 10 min window, then a
   new `NoticeKind::SleepReminder { hour }`. Independent of the story feature;
   zero work between hours. Cost: one more timer id and a small state machine
   (copied from the hour story's `hour_wait`).
2. Share `TIMER_MASCOT_HOUR`. No new timer, but that timer only lives while a
   story may play (motion Full, no Energy Saver, preset not Mati, a story
   ready), so the reminder would vanish in those cases, and the two features
   tangle.
3. Check on an existing tick (date rollover, the notch clock). The clock tick
   only runs while the clock shows; the hour story lost hours exactly this way
   and moved to its own timer ([[mascot-hour-schedule]]).

The owner left the how to me ("u atur terbaik gimana"); approach 1 is my call.

## Rules (behaviour)

Timer: armed while `config.sleep_reminder && config.robot` for the next hour in
{1, 2, 3, 4} at hh:00:01 (same 1 s margin as `HOUR_ARM_MARGIN`; after 04:00 the
next is 01:00 tomorrow). Re-synced on: the toggle, the mascot on/off, a clock
jump or wake (`on_clock_jump`: stop + sync), and after each hour is done. A
clock jump or a wake re-arms; if the clock is still inside an hour's window that
is not yet done (shown or dropped), it tries again at once; otherwise the next
hour (no catch-up after the window).

On fire, the reminder shows at the first check where all of these hold, checked
now and then every 30 s until 10 min past the hour (`HOUR_WINDOW`,
`HOUR_RETRY`):

- the local time is inside [hh:00, hh:10) for hh in 1..=4 (a late fire after a
  wake or clock change after the window shows nothing; no catch-up)
- session not locked and display on (`screen_away()` false)
- no fullscreen window
- no recording (sound or screen)
- the owner is at the PC: last input within 10 min
- no menu open, no other timed notice on screen (the reducer's `current`)
- no panel open, except on the last try of the window (from 9:30 past the
  hour on): then it shows over a panel too (a latched panel can stay open for
  the whole window)

A sticky notice (`ClaudeWaiting`, `Reminder`, `StickyReminder`,
`UpdateAvailable`, `MoleFailed`) never blocks: a timed notice already covers
it and it comes back afterwards. A `ClaudeWaiting` left waiting at 1 AM must
not eat every reminder of the night.

"Done for this hour" means the reminder was actually shown: after handing it to
the reducer, check that `state.view()` shows it; if not (fullscreen began in
the same instant), retry in 30 s like any other gate.

Why every gate waits instead of skipping (unlike the hour story, which skips on
lock/fullscreen/recording): the owner asked that every hour has its reminder; an
unlock at 02:04 or a game closed at 02:06 still gets it. After 10 min it is
dropped for that hour.

Music playing: shows over the music view like any notice. A running mascot
story: the notice ends it the usual way (permanent gate), no wait. The reducer
also drops a `SleepReminder` in fullscreen (same as `Greeting`), as a backstop.

After the notice ends, no mascot story is started (`greeting_ended` matches
`Greeting` only; keep it so).

## Motion

New faces in `ui/robot.rs` `Expr`: `Drowsy` (01), `Yawn` (02), `Doze` (03);
04 uses `Sleep`. `NoticeSort::SleepReminder(hour)` → `choose_expr` → that face.

- Still pose (motion not Full, Energy Saver): Drowsy = half-closed eyes;
  Yawn = mouth / beak open; Doze = eyes closed, head sunk (`nod_dip` / tilt);
  Sleep as today. These must read clearly as stills.
- Moving (motion Full): a one-shot on the frame clock started by the new
  notice (`Brain`, like the greeting wave and the falk hop; nothing after the
  6 s): Drowsy blinks slowly twice; Yawn opens over ~0.8 s, holds ~1 s, closes;
  Doze sinks over ~2.4 s then jerks back up, twice; Sleep is still.
- Per mascot: the cat has no mouth today, so a yawn mouth is new art (a dark
  oval under the eyes); the falk opens its beak (`beak_open` exists) and its
  crest droops toward the Sleep angle as the hour goes up; the octopus gets the
  same dark oval mouth and its arms curl more each hour, coiled at 04.
- Accessories ride along as on every face.

## Constraints

- Rust 2024, Windows 11; `cargo test --locked` green. Tests on drive Z: fail on
  this PC; timing tests may flake under load (re-run alone).
- New texts only from glyphs the fonts cover (`ui::font_coverage`); the table
  above is plain ASCII.
- Config: new `sleep_reminder: bool` (default true) in `Config`, `merge_owned`,
  and `config_transfer::EXPORTED`.
- Timer id: pick a free one and add it to the unique-ids test (ids 42-47 are
  taken; check).
- Disk on D: is tight and other sessions build in parallel: share one target
  dir, run focused tests per task, the full suite once after merging.

## Decided by me (owner may override)

- Default on.
- Every gate waits up to 10 min, none skips at once (reason above).
- Sticky notices never block; an open panel blocks until the last try of the
  window, then the reminder shows over it.
- The reminder shows even right after the Night greeting; both may appear
  back to back when the owner unlocks at 01:0x.
- Badge when the mascot cannot draw: the greeting's tint with a moon icon if
  one exists, else the greeting's.
- Demo: `--demo` notice `SleepReminder` with `DI_SLEEP_HOUR=1..4`.

## Left to the builder

- Exact curves and lid / mouth sizes inside the clamp tests.
- Where the per-hour pose progress lives in `Brain`.
- The pure helper that makes the hour story skip 1-4 (arm-time skip preferred
  over a skip at fire time).
