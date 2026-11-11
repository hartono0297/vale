---
tags: [gotcha, screen-recording, win32]
date: 2026-11-11
---

# Keeping new windows out of a capture needs an in-context WinEvent hook

`SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` must be on a window before
DWM composes its first frame, or that frame lands in the video. An
out-of-context `SetWinEventHook(EVENT_OBJECT_SHOW, .., WINEVENT_OUTOFCONTEXT)`
is too late: the event reaches the hooking thread only at its next
`GetMessage`, after the handler that created and showed the window has
returned (Vale's device dropdown paints, then shows, in one handler). Adding an
out-of-context `EVENT_OBJECT_CREATE` does not help for the same reason.

What works (Windows 11, tested): an in-context hook on Vale's own process with
the exe's own module, `SetWinEventHook(EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW,
GetModuleHandleW(None), cb, own_pid, 0, WINEVENT_INCONTEXT)`. The callback runs
inside `CreateWindowEx` / `ShowWindow` on whichever thread raised the event, so
the affinity is set before the window is first shown; it also catches windows
of worker threads (file dialogs). Install the hook first, then `EnumWindows`
for the windows already there. The list of changed windows is process-wide
under a lock that is never held across a Win32 call (the callback can re-enter
on the same thread). If the in-context hook is refused, `src/screenrec/exclude.rs`
falls back to the out-of-context SHOW hook and logs it.

Windows accepted the affinity on a window never shown. Only windows whose
affinity was `WDA_NONE` are touched and restored. Related: [[screen-recording]].
