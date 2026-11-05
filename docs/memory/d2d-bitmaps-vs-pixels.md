---
tags: [gotcha, direct2d, widgets, performance]
date: 2026-11-05
---

# D2D bitmaps belong to a render target, CPU pixels do not

An `ID2D1Bitmap` is made by one render target and is useless on another, so a
recreated canvas (`Canvas::with_gfx`) loses every bitmap. The decoded pixels
(WIC decode plus Fant scale, the expensive part) are plain memory and can be
kept and uploaded again cheaply.

The sticky note's image cache first threw everything away with the canvas, and a
resize recreates the canvas on every pointer move, so each move decoded every
photo again on the UI thread. Keep pixels and bitmaps apart: `ImgCache::decoded`
survives, `ImgCache::drop_bitmaps` clears only the bitmaps. Also: the key
includes the drawn size, so pick a stand-in size while the size is changing
(`paint::image_plan`) instead of decoding per move.

Details and the policy are in [[widgets]] ("Image cache policy"); the index entry
is [[index]].
