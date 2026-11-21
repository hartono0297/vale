---
tags: [decision, sign, images, memory]
date: 2026-11-21
---

# Sign: one pixel cap for images

One cap of 64,000,000 px for signing source images, used by open, `decode_oriented` and compose. Over it, the window refuses at open with a clear "too big" message, never a failure at save. Before the final fix the caps disagreed (open 250 MP, compose 40 MP), so 40 to 250 MP images opened but never saved.

- 64 MP covers 48 and 50 MP phone photos and a 600-dpi A4 scan; a 600-dpi A3 scan (69.6 MP) is refused. It also bounds save-time memory (the compose peak is several times the image).
- **Maker photos** (signature from a photo): decoded through the WIC scaler to at most 1600 px on the long side, source cap 250 MP.
- Orientation (EXIF) is applied once in `image_out::decode_oriented`, and the render path uses the same, so display, geometry and output agree.

Part of [[tanda-tangan]]; see also [[sign-signature-store]].
