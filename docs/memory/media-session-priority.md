---
tags: [decision, media]
date: 2026-11-01
---

# The island shows the best media session, not Windows' current one

Windows names one "current" session, which is often a browser tab playing a
video or a social feed while a music player sits paused or even plays. The
watcher (`src/sources/media.rs`) now follows every GSMTC session, ranks them,
and shows the best.

## Ranking (`choose`)

Closed and stopped sessions are never chosen. Of the rest the best by the tuple
(playing, music, Windows' current session, the one shown before). A full tie
takes the first in list order. Consequences:

- A playing session beats a paused one, so a paused player gives way to a
  playing video, and takes the screen back the moment it plays again.
- Among playing (or among paused) sessions, music beats video and social tabs.
- Stickiness: on a tie Windows' current session wins; with no current one the
  session shown before stays, whatever the list order, so the island does not
  flip between two equal tabs.

"Playing" for ranking is wider than what the island displays (`rank_playing`,
`ranked_live`): a session reporting `Changing` (a skip) ranks as playing, and
the shown session keeps ranking as playing for `HOLD` (2 s) after it goes from
playing to paused, opened or unreadable. Without this, a skip or the short
pause between two tracks handed the island to another playing session (say a
muted video) for a moment and back, resetting lyrics and the cat. The hold ends
early when it plays again, closes or stops, or another session is chosen; when
it runs out, `next_refresh` times out and asks for a re-rank, so a really
paused player gives way without waiting for another event. The play/pause
button still shows the real status at once.

## What counts as music (`is_music`)

A known player's AUMID (`KNOWN_PLAYERS`, case-insensitive substring: Spotify,
Apple Music, iTunes, Tidal, Deezer, Amazon Music, YouTube Music desktop,
SoundCloud, Pandora, foobar2000, MusicBee, AIMP, Winamp, Groove/Media Player,
Dopamine, Plexamp), or both artist and album filled in. An artist alone is not
enough: a video tab often has its channel as artist. Browsers are not in the
list. All tabs of one browser share one AUMID, so artist + album is the only
signal that tells a music tab from a video tab.

Gotcha: the declared `PlaybackType` is useless and not read. Browsers always
report Music: Chromium and Firefox hard-code it for every media session, and
so does every Electron or WebView2 host. A live YouTube tab in Chrome showed
`PlaybackType Music`, an artist and no album. Filtering browsers by AUMID does
not work either: Firefox's AUMID is an install-path hash (e.g.
`308046B0AF4A39CB`), not "firefox".

## Subscriptions and reads

- Every tracked session subscribes `MediaPropertiesChanged` and
  `PlaybackInfoChanged` (signals carry the session's key); `TimelinePropertiesChanged`
  only on the shown session, moved when the shown session changes.
- Any playback or properties signal of any session re-ranks. A timeline signal
  only re-reads the timeline. Signals of sessions no longer tracked are dropped
  (`Watcher::accepts`). `next_refresh` still folds a burst into one `Refresh`.
- Statuses are cheap and are re-read for all sessions on a re-rank. Metadata
  (title, artist, album) is read for new sessions and for sessions whose own
  properties signal fired. The thumbnail is read only for the shown session,
  when it becomes shown or its own properties change. A failed read keeps the
  old values (a new session with none counts as non-music, empty text).
- "New track" for the timeline (force an `Event::MediaTimeline`) is: the shown
  session changed, or its properties were re-read.
- Stale cover: when a session that is not shown has its metadata re-read and it
  owns the cached cover, the cache is marked stale (`art_for` cleared,
  `forget_stale_art`), so its cover is read again when it is next shown. The
  shown session of the moment never shows it (`info` pairs art with `art_for`).
- Keys come from a counter starting at 1 and are never reused; a session is
  recognised across re-lists by its COM identity. 0 means unknown.
  `GetSessions()` returns the same object for a session on every call, so
  this is stable.

## Gotcha: the current session is a different COM object

`GetCurrentSession()` hands back a different COM object than the
`GetSessions()` entry for the same session (checked live: one Chrome session,
two different `IUnknown` pointers, each stable on repeated calls). Matching it
by identity alone found nothing, so the "Windows' current session" tiebreak
never applied. `Watcher::current_key` still tries identity first, then
`match_current`: the only tracked session with the current one's AUMID; when
several share it (browser tabs), the only one of those whose status, title
and artist equal the current session's (read only in that case). An empty
AUMID, a failed read or several equal tabs give no current session, and the
tie falls to the session shown before.

## Commands go by key

`MediaInfo::session_key` is the shown session's key. The watcher publishes its
tracked sessions to a process-global registry (`SESSIONS`) before the
`Event::Media` that can carry a new key goes out. `send_command(cmd, Target {
key, app }, click)` clones the session for that key out of the registry (the
lock is never held across a WinRT join) and sends the explicit TryPlay /
TryPause / skip. Key 0 or an unknown key sends nothing. Because the key is
exact, two tabs sharing an AUMID are told apart; the click guards in
[[media-click-guards]] are unchanged.

## Log

When the chosen key changes (also to or from none) one line is written:
`<stamp> choose app=<AUMID or -> of=<live sessions>`. The AUMID only, never a
title, artist or album. See [[media-diagnosis-log]].

Not covered by tests: the WinRT side (relist, subscriptions, registry, the
identity match of the current session); the
ranking, music rule, current-session fallback, signal coalescing, key
lookup and the stale-art rule are unit tests.
`--probe-media` (debug builds) prints source app and key per update.

Listed in [[index]].
