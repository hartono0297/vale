---
tags: [gotcha, windows, transcript, gpu]
date: 2026-11-09
---

# Defender toast on the first GPU transcript

The first GPU job of a run can raise a Windows Security toast "Unauthorised
changes blocked" (Defender event 1127). Cause: the AMD Vulkan driver
(`amdihk64.dll`) changing memory inside `vale.exe`, which Controlled folder
access blocks. The transcript still completes. Vale mirrors the toast in the
notch like any other notification.

Not Vale's code to fix. The owner can allow `vale.exe` in Controlled folder
access. Same family as [[controlled-folder-access]]. Seen in the live check of
[[transcript]] on an AMD GPU, 2026-11-09.
