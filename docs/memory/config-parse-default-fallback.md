---
tags: [gotcha, config]
date: 2026-11-13
---

# `config::parse` turns one bad field into a default config

`config::parse` is `try_parse(..).unwrap_or_default()`. Only some fields have
lenient readers (notch size, hotkeys, the prefs structs, …); `style`, plain
`bool`s, `battery_threshold: u8` and enums given a non-string or `null` fail the
whole deserialise. One such value makes the result `Config::default()`: the
Google client, `muted_apps`, own tiles, folders and `onboarding_done` are gone.

For `config.json` itself the save path backs an invalid file up first, so a
hand edit loses nothing on disk. Code that builds a config from outside data
must not lean on `parse`: check each key on its own and finish with
`try_parse`, refusing on `None`. The settings import does this
([[settings-transfer]]); the final review caught it by probing
`{"robot":"yes"}` and `{"style":null}`.
