---
tags: [decision, gotcha, lyrics, lrclib]
date: 2026-11-01
---

# Lyrics lookup: retry, steps, consensus pick

Builds on [[lyrics-sync]] (which describes the first search fallback and the
`.none2` marker; both are superseded here).

## LRCLIB is often busy

Measured live 2026-10-31: `/api/get` and `/api/search` both answer **503**
`{"name":"ServerOverloaded"}` with `retry-after: 1` fairly often, and the same
request 1-2 s later works. The old `lookup` read a 503 as `Failed` and only
ran the cleaned-title step and the search on a definitive "none", so one 503
skipped every fallback.

- `Http::get_with_headers_capped_retry` returns `(status, body,
  Option<Retry-After>)` (`get_with_headers_capped` calls it).
- Every request goes through one helper: a 429 or any 5xx waits
  `retry_delay` (the header's whole seconds, clamped to 1..=3 s; missing,
  unparsable or an HTTP date = 1 s) and asks the same URL again, at most 2
  retries. A network error or timeout is asked once more. After that the URL
  is **transient**.
- `LOOKUP_BUDGET` is 30 s from the start of the network part, checked before
  every request and every sleep; past it nothing more is sent and what is left
  counts as transient.

## Steps (`lookup_chain`)

Each runs until one yields an accepted synced candidate; a failed or empty
step never stops the next. The chain is written against a `Net` trait (get,
sleep, now) so the tests script it with a fake clock.

1. `/api/search?track_name&artist_name` with the cleaned names.
2. `/api/get` with the raw title, artist and length, then with the cleaned
   names if they differ.
3. `/api/search?q=<artist> <title>` (free text, cleaned names), then the same
   with the title's trailing "(...)" / "[...]" group cut. Covers: "Red
   (Taylor Swift)" by Against The Current is found only by `q=Against The
   Current Red`. A URL already asked is skipped.

## Outcome rules

- An accepted candidate: `Answer::Synced` (cached by `keep_answer` as before).
- Nothing found and **every** request answered definitively (200, 404, other
  4xx such as 400): `Answer::None`, the 24 h negative marker (a provisional
  query still does not write it).
- Nothing found and any request transient (429/5xx after retries, an error, an
  unreadable or over-size 200, or the budget ran out): `Answer::Failed`, the
  existing retry timer, never a marker. A transient step must not be
  remembered as "no lyrics for a day".

## "Artist - Title" split

Plain YouTube videos look like title `Avril Lavigne - When You're Gone
(Official Video)`, artist `AvrilLavigneVEVO`. `split_artist_title` splits the
cleaned title on its first " - " when the first part equals the cleaned artist
(case-insensitive, letters and digits only: "Avril Lavigne" == "AvrilLavigne")
or the raw artist ends with "VEVO" or " - Topic". The pair is then the
"cleaned names" of steps 1, 2 (second get) and 3. Titles such as "Red (Taylor
Swift)" and "Some Song - Live" are not split.

## Best synced version: consensus and intro shift

LRCLIB entries are user uploads and their timing varies; copies of one
timing agree with each other. Two real cases (fixtures in `src/testdata/`,
used by the tests in `lookup_tests`):

- **Ed Sheeran "Perfect"**, official video, 282 s. All 20 entries share the
  album timing (first line at 2.98 s), lengths 201..280, lower median 263 s.
  The old pick kept only the 280-s entry (within 3 s), so every line showed
  ~19 s early: the video has a ~19 s longer intro than the album.
- **Five for Fighting "Superman (It's Not Easy)"**, 219 s. Within 3 s there
  are several timings for "I can't stand to fly": 18.41 (the three 219-s
  entries), 20.10 .. 21.29. Weighing the whole 3 s window picked 21.29: ~2.9 s
  late. The exact-length entries agree and are right.

`pick_best` (a search reply, `many`; the first 20 synced entries):

1. **Tight window.** A candidate's gap is `|length - playing|` (a missing
   length counts as 0 off). If any candidate has gap <= `TIGHT_DURATION_MS`
   (1.5 s), only those are weighed; else those within `SEARCH_DURATION_MS`
   (3 s); else the all-lengths fallback (step 4).
2. **Consensus** among the weighed ones: lines are matched by normalised text
   (the k-th occurrence with the k-th, first 300 lines, at least 3 shared
   lines per pair); each candidate's score is the median of its absolute
   shifts to the others. Lowest wins; ties go to the closest length, then
   the first. A shifted upload loses even when its length is the closest.
3. **Intro shift** of the winner C. Its family is every candidate of the
   reply, any length, C included, whose timing is within `FAMILY_MS` (500 ms)
   of C's (measured against C, not chained). The reference is the lower
   median of the family's lengths (none known: no shift). When
   `playing - reference` is over 3 s and at most `MAX_INTRO_MS` (60 s), every
   line is shifted **later** by that difference; a shorter playing version is
   never shifted. Perfect at 282 s: reference 263 s, +19 s. Superman at 219 s:
   the family is the three 219-s entries, no shift. Superman at 222 s: no
   shift (the 220-223 s entries agree within a second).
4. **All-lengths fallback** (no candidate within 3 s): the consensus runs over
   all candidates and the winner is returned **only** when step 3 shifts it;
   otherwise `None` and the next step runs: a playing version that is shorter,
   or longer by 3 s or less, or longer by over 60 s (Perfect at 345 s) gets
   nothing from this reply.

A `/api/get` reply (a single entry, no family) works as before
(`pick_exact`): returned when its length is within 3 s of the playing one (a
missing length counts), no tight window, never shifted.

The returned `Answer::Synced` text is the shifted LRC (`format_lrc`: one
`[mm:ss.xxx]text` per line, a gap is just its tag), so the disk cache, the
memory cache and the display agree. An unshifted pick returns the upload's
text unchanged. There is no user offset setting on purpose (removed in v3.9).

## Cache version: `.lrc2`, `.none4`

`LRC_EXT` is `lrc2` and `NONE_EXT` is `none4`. Old `<hash>.lrc` and
`.none` .. `.none3` files are ignored (never read, not cleaned up), so every
track is looked up once more with the new pick: lyrics cached by the old
choice may be early or late, and a music video with a `.none3` marker may now
get shifted lyrics. Markers keep their rules: 24 h TTL, removed when expired
or future-dated, and `write_lrc` removes the marker.
