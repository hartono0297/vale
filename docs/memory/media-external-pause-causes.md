---
tags: [gotcha, media, debugging]
date: 2026-10-27
---

# Music pauses by itself: look outside the app first

If music pauses and the island sent no command, the cause is usually outside the app:

- YouTube Music shows its own idle prompt, "Video paused. Continue watching?", and pauses until someone answers it.
- Bluetooth headsets pause playback through ear-detection sensors or their buttons.
- Teams and Zoom pause media during calls.

The island does not try to dismiss browser prompts. That is a deliberate choice.

## How to tell

1. Run with `--media-log`, or set `"media_log": true` in `%APPDATA%\Vale\config.json`.
2. Read `media.log`. At about 1 MB it rotates to `media.log.1`. Track titles are never written to it.
3. Find the playback change to Paused. If no island command line comes just before it, something outside the island paused the music.

If an island command line does come just before it, check the click guards in [[media-click-guards]].

Listed in [[index]].
