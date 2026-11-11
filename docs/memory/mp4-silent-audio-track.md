---
tags: [gotcha, screen-recording, media-foundation]
date: 2026-11-11
---

# An MP4 audio track that never gets a sample makes the file unreadable

With Media Foundation's sink writer, an `.mp4` whose AAC stream was added but
never received a sample (only `SendStreamTick`s) finalizes, but reading it back
fails with `0xC00D36C4` (MF_E_UNSUPPORTED_BYTESTREAM_TYPE). It happened when the
screen recording's sound could not start: the track is created up front by
`options_for`, before the mix starts.

Fix in `src/screenrec/mod.rs`: while the audio track has never had a sample,
gaps are filled with silence instead of stream ticks, and a recording that never
heard anything is filled with silence to its end. After the first real sample
the gaps are ticks again, as before. Check: `--screenrec-test 6 --no-audio-push`
reads back 181 video frames and the audio packets. Related: [[screen-recording]],
[[media-foundation-dynamic-load]].
