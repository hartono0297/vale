# Screen recording: video quality choice (design)

Date: 2026-11-12. A small change, settled in chat with the owner ("intinya ada
pilihan quality dan di setting dikasih tau estimasi size ny ke user", then
"atur ja yg terbaikny gimana": the details below are the builder's calls).

## Intent

The owner wants to pick how big a screen recording is (1080p and the like) and
see, in Settings, roughly how much disk a minute of video takes, before
recording. Today every video is the recorded thing's own size (capped at
4096 x 2304 by the encoder) and nothing says how big the file will be.

## What gets built

- **Settings → Fitur → Rekam layar** gets a first row, **Kualitas video** /
  **Video quality**, a segmented control: **Asli · 1440p · 1080p · 720p**
  (English **Original · 1440p · 1080p · 720p**). Default **Asli**, so nothing
  changes until the owner picks another. The card goes from 4 to 5 rows;
  "Gambar per detik" moves to the second row.
- **The estimate** is the row's description: `1920×1080 · ± 36 MB/menit`
  (English `1920×1080 · ± 36 MB/min`). It follows the chosen quality and the
  chosen 30/60 fps at once. Its size is what the **primary monitor** (the
  one in Settings' monitor list marked primary; 1920×1080 when the list is
  empty) records at that quality.
- **A choice is a cap on the shorter side**, aspect kept: 1080p on a
  2560×1440 screen is 1920×1080, on a 3440×1440 ultrawide 2580×1080, on a
  portrait 1440×2560 screen 1080×1920. No bars, no stretching.
- **Never scaled up**: a window or area whose shorter side is already under
  the cap keeps its own size (a 900×600 window at 1080p stays 900×600).
- The encoder's own cap (4096×2304) and the minimum side (64) still hold.
- **Applies from the next recording**: the start snapshots the choice, like
  the cursor effects. While a recording is busy the choice is refused, like
  30/60 fps.
- Zoom keeps working: it is drawn into whatever size the video has.

## The estimate's numbers (builder's call)

- Video bitrate stays the existing rule, `plan::bitrate` (0.08 bit per pixel
  per frame, 1 to 50 Mbit/s). "Quality" is resolution only; sharpness per
  pixel does not change.
- Audio is always counted at 96 kbit/s (the mono AAC track), so the number is
  a ceiling. The encoder is unconstrained VBR at that average, so a mostly
  still tutorial usually ends up smaller, rarely bigger.
- MB is 1,048,576 bytes (what Explorer shows), rounded to the nearest whole
  number, at least 1.
- Worked values (30 fps unless said): 1920×1080 → 36; 2560×1440 → 64;
  1280×720 → 17; 1920×1080 at 60 → 72; 2560×1440 at 60 → 127.

## Config

`recording.video_quality` in config.json: `"original"`, `"1440p"`,
`"1080p"`, `"720p"`. Lenient like the rest of `recording`: anything else or
missing reads as `"original"`.

## Not built

A bitrate / sharpness choice apart from resolution, per-recording overrides on
the picker, an estimate per monitor or per area, upscaling.
