---
tags: [gotcha, transcript, gpu, whisper]
date: 2026-11-19
---

# A GPU that writes only "!" in a transcript

Symptom: a transcript whose every line is `!!!!!!...` (owner's meeting,
2026-11-18, Whisper large-v3-turbo). Cause: on the Radeon 530 (Vulkan1) the
engine returns status OK with token 0 ("!") over and over, so the decoder's
logits are broken on that GPU. The same file on the CPU reads fine. Vale kept
the text because only `ERR_BACKEND` triggered the CPU reload; a 15 min run for
2 min of audio was the decoder never reaching end-of-text.

Fix (`engine::screen` after `Session::run`, shared by the transcript and
[[live-captions]]): a result that is 90 % "!" of 8+
non-blank characters (`is_junk`; only "!", so a music note, "....." or "aaah" is
never moved off the GPU) is `EngineError::Backend` on a GPU, so
`run_job` and the caption worker reload on the CPU like any GPU failure; on the
CPU it is dropped as empty. Whether a run was on a GPU comes from the `cpu` flag the model was loaded
with, then the backend name. Not caught: a chunk that goes bad only partway
("Selamat pagi !!!!"). Cost: a GPU like this still loads (20 to 60 s) and
fails on the first chunk of every job; nothing remembers it. Left for later:
remember the bad GPU in config and go straight to the CPU.

Old transcripts stay as they were: press Transcribe again. See [[transcript]],
[[transcript-defender-toast]] (same AMD Vulkan driver family).
