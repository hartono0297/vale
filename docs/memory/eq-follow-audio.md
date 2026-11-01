---
tags: [decision, audio, equalizer, power, timers]
date: 2026-10-31
---

# The equalizer bars follow the music

Owner request: the five bars beside the song title in the compact media view
should follow the song, not a sine. `src/eq_audio.rs` captures the default
output device in WASAPI loopback, splits it into five bands and publishes five
0..=1 levels; `geometry::audio_bar_heights` turns them into bar heights. The
sine (`equalizer_heights`) stays as the fallback and for paused media, the
demos and the Settings preview.

## The analysis (pure, tested)

- **Formats** (`parse_format`): float32 (tag 3, or EXTENSIBLE with the float
  subformat), PCM 16, PCM 24 packed in 3 bytes, PCM in a 32 bit container.
  Anything else is `None`: the open fails and the bars stay on the sine.
  `Win32_Media_Multimedia` is not enabled, so the format GUIDs and tags are
  written in the module. `WAVEFORMATEX` is packed: every field is read with
  `addr_of!(..).read_unaligned()`. `mix_to_mono` averages the channels to -1..1.
- **Bands** (Hz): 60-150, 150-400, 400-1000, 1000-3000, 3000-8000. One RBJ
  band-pass biquad per band (constant 0 dB peak gain, state in f64), centre =
  sqrt(lo*hi) clamped below 0.45*fs, Q = centre/(hi-lo), built from the
  stream's own rate.
- **Hop**: every 10 ms of samples (fs/100) each band's RMS closes a hop.
  `advance(dt, muted)` (once per UI tick) spreads `dt` over the hops that
  arrived; with none (loopback delivers nothing in silence) it runs one step
  towards zero, so the bars decay to rest.
- **Per hop and band**: gate (RMS below 0.0003 is 0), slow auto-gain
  (`peak = max(rms, peak*exp(-dt/3 s))`, `norm = rms / max(peak, 0.003)`),
  shape `norm.clamp(0,1)^1.5`, then smoothing `alpha = 1 - exp(-dt/tau)` with
  tau 30 ms rising (attack) and 200 ms falling (decay). Muted: the target is
  0 for every band (the peaks keep following the stream, so unmuting is
  instant).
- **Gotcha, auto-gain is per band.** A pure tone lights every band that has
  energy above the gate, since each band normalises to its own peak (a fresh
  analyzer gives 1.0 on the neighbours of a lone sine). Real music is
  broadband, which is what the gain is for (it lifts the highs against the
  bass). The "tone lights its own band most" test therefore plays the whole
  spectrum first, then the tone.
- **Heights**: `BAR_MAX_H * (EQ_REST + (1 - EQ_REST) * level)` with
  `EQ_REST = 0.2`; muted, or a level that is not a number, is rest.

## The worker (`EqCapture`, thread `eq-capture`)

- COM `CoInitializeEx(MTA)` once, like `audio-mixer` ([[mixer]], a second Core
  Audio worker). Shared state `EqShared { levels, live, muted }` (relaxed
  atomics); control by an mpsc `Sender<bool>` (`set_running`). The UI never
  blocks on the worker and the worker never wakes or posts to the UI: the
  existing `TIMER_EQ` tick (33 ms) is the only repaint, no timer was added.
- **Stopped**: the thread blocks in `rx.recv()`: no polling, no audio client,
  `live = false`. **Running**: `recv_timeout(15 ms)`, drain the control queue
  (the last message wins), drain the packets (a `SILENT` flag counts as zeros),
  feed the analyzer, publish with relaxed stores. The default render endpoint
  (`eRender`, `eMultimedia`, as `audio.rs`), shared mode, `LOOPBACK`, the mix
  format, a 100 ms buffer.
- **Mute**: every 250 ms `IAudioEndpointVolume` `GetMute` and
  `GetMasterVolumeLevelScalar` on the same device; muted or a master level of
  0.001 or less sets `muted`. Loopback is taken before the endpoint volume, so
  a muted or turned-down output still delivers the full stream: without this
  the bars would dance in a silent room.
- **Device switches**: every 1 s the default endpoint's id is compared with the
  open device's. A default switch (Bluetooth or USB headset) does not
  invalidate the old device, so waiting for `AUDCLNT_E_DEVICE_INVALIDATED`
  alone would keep capturing a device nobody listens to. (An
  `IMMNotificationClient` would avoid the 1 s read; it needs a COM object and
  callbacks on another thread, not worth it for this.) A changed id reopens at
  once; any capture error (including the invalidated one) reopens after
  250 ms.
- **Failure**: no device, an unsupported format or an init error sets
  `live = false` (the UI draws the sine) and is retried at most every 2 s
  while still running. Stopping drops the session (`Stop`, release) and sets
  `live = false`.
- **Lazy**: the thread is spawned the first time capture is wanted
  (`App::sync_eq`), so an island that never shows playing compact media never
  starts it. It is detached; dropping `EqCapture` ends it.

## When it runs (`app.rs`)

- `eq_timer_wanted(view_compact, view_lyric, dance_wanted, playing, eyes,
  fullscreen, screen_away)`: `TIMER_EQ` stops while fullscreen and while the
  session is locked or the display is off (`screen_away`). **It also runs in
  the lyric view while the cat dances there** (`dance_wanted` = robot &&
  "Joget ikut musik" && full motion && Windows animations,
  [[cat-dances-to-music]]), so loopback capture can now run in the lyric view
  too; the dancing cat reads band 0 (`App::eq_bass`) from the same shared
  levels. `eq_capture_wanted(timer_on, setting_on)`: the capture runs exactly
  while the timer does and "Equalizer ikut irama" (`eq_follow_audio`,
  Settings -> Tampilan -> Island, default on) is on. Both are tables in tests.
- `sync_eq` is called from `sync_timers` (every `refresh`) and from
  `sync_pointer_polls` (lock, unlock, display on/off), and sends
  `set_running` only when the wanted state changes.
- `Frame::eq_heights` is `Some` only while capture is wanted **and**
  `live`; `None` draws today's sine, so paused media keeps its static bars.
  The demos (`demo.rs`) set `None`, so they never capture; the Settings
  preview's frozen bars are untouched.
- Config key `eq_follow_audio`: lenient like `cat_follow_pointer` (junk reads
  as on), copied by `merge_owned`.

## Measured and not verified

- Release exe: 2,508,288 bytes before (the parent commit built in a folder of
  another name, see [[exe-size]] for the +-512 B path effect), 2,539,008
  after: +30,720 bytes (+1.2 %), no new crate, no new `windows` feature.
- **Not verified live** (the build must not touch the owner's audio or the
  running island): a real loopback capture, how the bars feel against a song,
  the 1 s device switch with a headset, a muted or turned-down output, an
  exclusive-mode app, the thread's idle cost, and the Tampilan row in the
  real window.
- Assumptions: rest at 20 % of `BAR_MAX_H`; the `eMultimedia` role; the 1 s id
  check instead of `IMMNotificationClient`; `TIMER_EQ` also stops while
  locked or with the display off.

Related: [[bata-idle-power-budget]], [[studio-media-progress-timer]],
[[settings-structure]], [[exe-size]].

Listed in [[index]].
