---
tags: [gotcha, windows, media-foundation, build]
date: 2026-11-09
---

# Media Foundation is loaded at run time, never imported

Windows N editions ship without Media Foundation unless the Media Feature Pack
is installed. A static import of `mfplat.dll` / `mfreadwrite.dll` would stop
`vale.exe` from starting at all there. So `src/mf.rs` loads them with
`LoadLibraryExW(LOAD_LIBRARY_SEARCH_SYSTEM32)` and `GetProcAddress`, and a
missing DLL is `EncodeError::NoMediaFoundation` ("Media Foundation tidak ada
di Windows ini"), only for the recording and the transcript.

Since the transcript ([[transcript]]) there is one loader: the recorder's
encoder and the transcript decoder both use `src/mf.rs`. `MfSession` does the
COM MTA init and `MFStartup`, so callers no longer init COM themselves. Track
C's H.264 writer should use it too. Related: [[recording-audio]].
