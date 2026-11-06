---
tags: [gotcha, direct2d, power]
date: 2026-11-06
---

# A DC render target copies the whole bound rect every frame

`ID2D1DCRenderTarget::BindDC(dc, &rect)` makes a software target read the whole
rect back from the DC at `BeginDraw` and write all of it to the DIB at
`EndDraw`: two full-surface GDI copies per frame in win32k, whatever clip is
pushed in between. The island bound its whole surface every frame, so a dirty
rect (clip plus `prcDirty`) saved the painting but not the copies: main thread
86 ms CPU/s, 59 ms/s of it kernel time, with music, the lyric view and the
dancing cat (measured 2026-11-06).

Fix: draw on a persistent software WIC bitmap target (`CreateWicBitmapRenderTarget`)
the size of the surface and memcpy only the dirty rows into the DIB that
`UpdateLayeredWindow` reads. No DC is bound, read back or selected per frame.
The target lives as long as the surface size and takes the D2D bitmap caches
with it when it goes ([[d2d-bitmaps-vs-pixels]]).

Details, the self-check and the numbers: [[bata-idle-power-budget]]
("Dirty-rect present and the EQ skip", "The WIC bitmap target"). Index: [[index]].
