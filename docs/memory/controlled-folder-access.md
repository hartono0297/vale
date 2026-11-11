---
tags: [gotcha, windows, files, recording]
date: 2026-11-07
---

# Controlled folder access refuses Vale in Documents (and Videos)

Windows Security's ransomware protection ("Controlled folder access", off by
default, on on the owner's PC) lets only apps Microsoft trusts write in
Documents, Pictures, Videos, Music, Desktop and Favorites. `vale.exe` is
unsigned and new, so it is refused. Code signing alone does not fix it (the
allow list follows Microsoft's reputation), and an app must not add itself to
the allow list (admin only, a security setting).

- **It does not say "access denied".** Creating a folder or a file there
  fails with os error 2 or 3 ("not found") although the drive exists, so
  `recorder::refused` reads not-found on an existing drive as blocked.
- **What recording does** ([[recording-audio]]): the `rec-start` thread
  probes the folder (create it, create and delete a probe file). When refused
  it records into `%USERPROFILE%\Vale\Rekaman` (`prefs::fallback_folder`; the
  profile folder itself is never guarded by default). The default folder then
  moves there for good (config `recording.folder`), a folder the user picked
  stays theirs, and a notice says "Windows melindungi folder rekaman · Disimpan
  ke <user>\Vale\Rekaman · ganti di Pengaturan". Only when the fallback is
  refused too does the start fail with the blocked-folder notice (click opens
  the folder picker).
- **Screen recording (track C) must do the same** for `Videos\Vale`.
- Owner fix on one PC: Windows Security → Ransomware protection → Allow an app
  through Controlled folder access → add `vale.exe` (by path; a dev build in
  `target\release` needs its own entry).
