---
tags: [decision, robot, greeting, config]
date: 2026-10-31
---

# The greeting follows the time of day

Vale v3.8: the cat's greeting is no longer a morning-only, once-a-day line.
`src/greeting.rs` (pure, tested) splits the local day into five periods and
the greeting shows **at most once per period per calendar day**. A later
period the same day greets again. The noon limit of [[bata-morning-greeting]]
is gone.

| Period | Hours | Face | Title (ID / EN) | Body counts |
|---|---|---|---|---|
| Morning | 04:00-10:59 | Wave | Selamat pagi / Good morning | today's events + undone reminders |
| Midday | 11:00-14:59 | Happy | Selamat siang / Good afternoon | left today |
| Afternoon | 15:00-17:59 | Wave | Selamat sore / Good afternoon | left today |
| Evening | 18:00-23:59 | Happy | Selamat malam / Good evening | tomorrow's events |
| Night | 00:00-03:59 | Sleep | Masih begadang? / Up late? | the current date's events |

- "Left today" = today's events that have not ended (all-day ones count) +
  undone reminders due at or after now.
- A zero part is dropped ("2 pengingat hari ini", never "0 acara"); when all
  is zero the body says so ("Sisa hari ini kosong", "Besok belum ada acara").
  Texts are fixed and bilingual (`i18n::greeting_title` / `greeting_body`),
  not editable. Evening and Night count events only.
- Night assumption: after midnight the user has not slept yet, so "besok" is
  the **current calendar date**, not the next one ("Sudah larut. Besok 2 acara").

## Code

- `greeting::period_at(NaiveTime)`, `greeting::face(period)`.
- `robot::should_greet(last, now, settings) -> Option<DayPeriod>`: robot and
  greeting on, and `last != Some(GreetMark { today, period_at(now) })`.
- `NoticeKind::Greeting { period }`; `choose_expr` returns `greeting::face`.
- `app::greeting_counts(events, reminders, now, period)` and `maybe_greet`.
- Demo: `DI_GREET_PERIOD=morning|midday|afternoon|evening|night` picks the
  greeting shown by the demo notice.

## Storage

`config.last_greeting` is one string, `"2026-10-31 evening"` (period ids
`morning|midday|afternoon|evening|night`). Reading is lenient: an old plain
`"2026-10-27"` reads as that date's Morning (already greeted); anything
unreadable (bad date, unknown period, a number, null) reads as none and never
fails the rest of the file.

The Night greeting and the sleep reminder ([[sleep-reminder]]) are separate
notices and can come back to back at 01-03.

## No new timer

Triggers are unchanged: startup after the first calendar snapshot, a wake by
the user, session unlock (`greet_may_try`, fullscreen rule). So a period that
starts while the island runs greets at the next wake or unlock, not by itself.

Related: [[bata-morning-greeting]], [[bata-robot-expressions]]

Where the periods are shown in Settings: [[settings-structure]] (Kucing, Sapaan).

Listed in [[index]].
