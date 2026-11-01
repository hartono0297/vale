---
tags: [decision, media, input]
date: 2026-10-27
---

# Media clicks are guarded and play/pause is explicit

Rules that keep the island from pausing music the user did not mean to pause:

- A click counts only when press and release land on the same target on the island. A press that ends somewhere else does nothing.
- After a hover-opened panel finishes its open animation, media buttons ignore clicks for 300 ms. A pointer that is still moving cannot hit play/pause by accident.
- A panel opened by a click (Click mode, or a notice that leads to the panel) is guarded the same way, and keeps the guard while it is latched even if the click came before the hover dwell (`hover_opened` is cleared only when the panel closes, or when neither hover nor the latch holds it).
- The middle button plays or pauses only when its press and its release are both on the island ([[gestures]]).
- Play/pause sends an explicit `TryPlay` or `TryPause` to the session the island shows, never a blind toggle on `GetCurrentSession()`. The session is found by the key the watcher gave it (`MediaInfo::session_key`), not by AppUserModelId, so two browser tabs sharing one AUMID cannot be mixed up ([[media-session-priority]]).

The progress timer for that same session is described in [[studio-media-progress-timer]]. When a pause has no island command behind it, see [[media-external-pause-causes]].

Listed in [[index]].

- A session without an AppUserModelId is still controlled: the key identifies
  it (`pick_session`). Key 0 (nothing shown, demo) or a key that is no longer
  tracked sends nothing.
