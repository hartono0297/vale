---
tags: [gotcha, windows, media-foundation, build]
date: 2026-11-07
---

# Media Foundation is loaded at run time, never imported

Windows N editions ship without Media Foundation unless the Media Feature Pack
is installed. A static import of `mfplat.dll` / `mfreadwrite.dll` would stop
`vale.exe` from starting at all there. So `recorder::encode` loads them with
`LoadLibraryExW(LOAD_LIBRARY_SEARCH_SYSTEM32)` and `GetProcAddress`, and a
missing DLL is `EncodeError::NoMediaFoundation` ("Media Foundation tidak ada
di Windows ini"), only for the recording.

Track B (decoder) and track C (H.264 writer) have their own loaders; unify
them into one when they merge. Related: [[recording-audio]].
