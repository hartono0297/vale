---
tags: [memory, decision, media]
date: 2026-10-27
---

# Media diagnosis log and the pause guards

Music sometimes paused by itself (Run 11). Without a reproduction the island got
three guards, plus a log for telling the island's own commands apart from
outside causes, such as YouTube Music's "Continue watching?" prompt, headset
ear sensors or call apps.

- **Turn the log on:** start with `dynamic-island.exe --media-log`, or set
  `"media_log": true` by hand in `config.json`. The field is hidden: it is not
  in Settings and is not written to the file while off. The log goes to
  `media.log` next to `config.json` (the folder comes from `config::path()`).
  At about 1 MB the file is renamed to `media.log.1`.
- **Lines:** `command`: hit, command (or `ignored(guard)` / `ignored(no media)`),
  page, `hover_open` age, target AUMID, and whether a session was found.
  `status`: AUMID and playback status of the shown session, written only when
  that pair changes. `choose`: the AUMID the watcher now shows (`-` for none)
  and how many sessions were live (`of=`), written when the shown session
  changes ([[media-session-priority]]). Titles, artists and albums are never
  logged.
- **Reading it:** a `status ... Paused` with no `command` just before it means
  something outside the island paused the music. A `command ... cmd=Pause`
  means a click on the island did.

Guards:
- A click needs both `WM_LBUTTONDOWN` and `WM_LBUTTONUP` on the island, on the
  same target (`ClickPair` in `app.rs`). A pointer leaving the island, a view
  change or a hidden window clears the press.
- After hover opens the panel, Prev/Play/Next do nothing for `ANIM_MS` + 300 ms
  (`MEDIA_HOVER_GUARD` in `state.rs`). A panel held open by an editor or the
  CLI input has no guard.
- Play/pause sends an explicit `TryPlay`/`TryPause`, based on what the island
  shows, to the shown session itself, found by `MediaInfo::session_key`.
  `source_app` (the AUMID) is only what the log prints. With no session for the
  key, nothing is sent.

Related: [[studio-media-progress-timer]], [[media-click-guards]], [[media-session-priority]]
