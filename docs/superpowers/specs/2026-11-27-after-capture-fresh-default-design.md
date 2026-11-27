# After capture: "On screen" by default for new installs

## Intent

The owner wants a new user's first screenshot to stay on the frozen screen
(Settings → Clipboard → *After capture* = *On screen*, `AfterCapture::Inline`)
instead of opening the editor window. People who already use Vale keep what
they have.

## Goals

- A first run (no `config.json`, or a blank one) starts with
  `screenshot.after_capture = Inline`.
- Everything else about a first run stays as it is.

## Non-goals and anti-goals

- No change for existing users: a saved value is kept, and a config file
  without the field (written before the setting existed) still reads as
  `Editor`.
- No change to `ScreenshotSettings::default()` or the `after_capture`
  deserializer: changing them would also move old configs without the field
  and the corrupt-file fallback.
- No UI, wording or migration change.

## Approaches

1. **Set it in `Config::fresh()`** (picked). The first-run config already lives
   there (Notch style, page and tile lists). One field, no effect on any file
   that exists.
2. Change `ScreenshotSettings::default()` to `Inline`. Rejected: older configs
   without the field and the corrupt-file fallback would switch too.
3. Add a "first run" flag and pick the mode at capture time. Rejected: more
   state for the same result.

## Design

- `Config::fresh()` sets `screenshot: ScreenshotSettings { after_capture:
  AfterCapture::Inline, ..ScreenshotSettings::default() }`.
- Tests: a fresh config (and `load_at` on a missing file) has `Inline`; a file
  without `after_capture` still reads `Editor` (existing test covers the
  default); the fresh value survives a save and reload.
- A corrupt file still loads `Config::default()` (`Editor`), as before.
