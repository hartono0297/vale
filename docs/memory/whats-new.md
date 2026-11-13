---
tags: [decision, update, settings, release]
date: 2026-11-13
---

# What's new per patch

After an update the notch says what changed, and Settings → Tentang has a card
**Apa yang baru** / What's new with the latest versions. The owner asked for
"What's new setiap patch" and left every choice to me. Spec:
`docs/superpowers/specs/2026-11-13-whats-new-design.md`.

- **Content** is a hand-written table, `src/whats_new.rs` (`ENTRIES`, newest
  first): per version 1-5 points in Indonesian and English. No network, no
  commit-generated text (the GitHub release notes are auto-generated commit
  lines in English and stay as they are).
- **Release guard:** the test `table_has_an_entry_for_the_running_version` fails
  when `Cargo.toml`'s version has no entry, so write the entry before bumping
  (AGENTS.md "Push and release" says so). `table_is_well_formed` holds the
  rest: versions descend, 1-5 points, none longer than 90 characters.
- **Memory of what was seen:** `config.whats_new_seen` (X.Y.Z, lenient read,
  kept by `merge_owned`, in `config_transfer::LEFT_OUT`: never exported). See
  [[config-parse-default-fallback]] and [[settings-transfer]].
- **When it shows** (`whats_new::start_action`, called from
  `app_update::update_start`, release builds only): `seen` empty on a fresh
  install (`!onboarding_done`) stores the version and stays silent; `seen`
  older, or empty/garbage on an existing install (an upgrade from a release
  without this feature), shows once and writes at once; a fresh install or downgrade is stored in memory only (`StartAction::saves_now`), so a start never moves a hand-edited, unparsable `config.json` aside; `seen` newer (downgrade)
  stores and stays silent; equal does nothing. Skipped versions are combined
  (`whats_new::since`); an unknown `seen` lists only the current entry. A
  `--imported` restart leaves it for the next start.
- **The notice** is the "Diperbarui ke X.Y.Z" one (for the updater's
  `--updated` start and any other start with a version change):
  `NoticeKind::WhatsNew`, body "old → new · klik untuk lihat apa yang baru",
  closable, 8 s (`UPDATE_NOTICE_DURATION`), a body click gives
  `Effect::OpenWhatsNew`, which opens Settings at `CardId::WhatsNew`
  (`CardId::section()` now says which section: `settings::open_card` is no
  longer Fitur only). Without a known old version the body is just the hint.
- **The card** sits between the Tautan card and "Pindah ke PC lain" in Tentang:
  one row per version of `whats_new::latest(3)`, the version over its wrapped
  "• point" lines. The existing Tentang layout tests index the cards from the
  end, so the Tautan card is third from the end now.
- **Demo:** `--demo whats-new` (made-up 9.9.9); the card shows in `--settings about`.

## Gotchas

- `app::record::tests::a_folder_is_writable_only_when_a_file_can_be_made_in_it`
  fails on a PC with a real `Z:` drive: it probes `Z:\no\such\drive`, which can
  be created there (and leaves empty folders). Unrelated to this feature; skip it
  there (`cargo test --locked -- --skip a_folder_is_writable`).

## Verified and not

- Seen on screen (debug build, `--settings about light en`, 2026-11-13): the
  "What's new" card under Links with the 5.3.1 row and its two points, text
  inside the card.
- Not seen: the notch notice (`--demo whats-new` was drawn under the owner's
  running island, so the screenshot showed only that one), the click that opens
  Settings from the notice (also with Settings already open on another
  section), a real update or first start with an old `whats_new_seen`, the
  notice body trimming with JetBrains Mono. Check these by hand.

Related: [[self-update]], [[settings-structure]].
