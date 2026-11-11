---
tags: [gotcha, audio, wasapi, recording]
date: 2026-11-07
---

# WASAPI capture: the QPC position is in 100 ns units, and loopback goes quiet

- `IAudioCaptureClient::GetBuffer`'s `pu64QPCPosition` is **not** in
  `QueryPerformanceCounter` ticks: it is the counter converted to 100 ns
  units. Compare it with `capture::qpc_hns()`, not the raw counter, or the
  mic and the PC audio land seconds apart.
- **Loopback delivers no packets while nothing plays** (not packets of
  zeros). A recording must place packets by their stamp and fill the gaps
  ([[recording-audio]]), and a PC-audio-only recording can end with nothing
  heard at all (`RecError::Silent`).
- The mic and the render device run on different clocks: expect drift of a
  few ms per minute, slewed one sample per packet.
- `E_ACCESSDENIED` on activation or `Initialize` of the mic is the Windows
  privacy switch: the notice opens `ms-settings:privacy-microphone`.

Related: [[eq-follow-audio]] (the other loopback user).
