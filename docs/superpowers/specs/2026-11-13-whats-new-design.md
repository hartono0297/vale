# What's new per patch: design

Small change. The owner asked: "gw mau ada 'What's new setiap patch'", then left
every choice to me ("u atur yang terbaiknya"). Every decision below is my call.

## Intent

After Vale updates, the user sees what changed in that version, every patch.
Today the notch only says "Diperbarui ke X.Y.Z" (`app_update::updated_notice`),
and the GitHub release notes are auto-generated commit lines in English.

## Goals

- A hand-written, user-facing list per version, Indonesian and English.
- Shown once after the version changes, however the update arrived (self-update,
  `install.ps1`, manual exe swap).
- Reopenable later from Settings.
- A release cannot be cut without the entry for its version.

## Non-goals / anti-goals

- No network fetch, no text generated from commits or GitHub notes.
- No new window, no blocking popup, no notice with new chip geometry.
- Not shown on a fresh install, in debug builds, or in the demos' real path.

## Approaches

- **A. Hand-written table compiled into the exe (picked).** Offline, controlled
  wording, testable.
- B. Read GitHub release notes at runtime. Needs network, text is commit-shaped.
- C. Generate from commits. Same text problem, more machinery.

## Design

1. **Content**: `src/whats_new.rs`, a static table, newest first:
   `Entry { version: &str, id: &[&str], en: &[&str] }`, 2-5 short points each.
   Pure functions: `since(seen, current) -> &[Entry]` (entries with
   `seen < version <= current`, using `update::parse_version`) and
   `latest(n)` for the card.
2. **Release guard (tests in the same file)**: an entry exists for
   `env!("CARGO_PKG_VERSION")`; versions parse and strictly descend; every entry
   has ID and EN points, 1-5 each, none empty. `cargo test --locked` already
   runs before the release commit, so a bump without a note fails. AGENTS.md
   "Push and release" step 2 gets one line: write the entry first.
3. **Memory of what was seen**: `config.whats_new_seen: String` (serde default,
   lenient read like the other fields, in the allowlist test of
   `config_transfer` as PC-only so an import never moves it).
4. **When it shows**: at a normal start of a release build (not debug, not
   demos): `seen` empty and no config file existed before this run
   (fresh install) -> store current, show nothing. `seen` empty with an
   existing config (upgrade from a version without this feature), or
   `seen < current` -> show the "updated" notice and store current at once, so
   it never repeats. `seen > current` (downgrade) -> store, show nothing.
   Skipped versions are combined: 5.3.1 -> 5.3.4 lists 5.3.4, 5.3.3, 5.3.2.
5. **The notice**: the existing "Diperbarui ke X.Y.Z" notice (both for
   `Start::Updated` and any other start where `seen < current`; one notice, never
   two) whose body becomes "old -> new · klik untuk lihat apa yang baru" / English
   twin. It gets a new `NoticeKind` with an action, like `ShotSaved`: a body
   click gives a new effect that opens Settings on Tentang scrolled to the new
   card (`settings::open_card` pattern). When there is no old version to show
   (non-self-update), the body is just "klik untuk lihat apa yang baru".
6. **The card**: Settings -> Tentang, a titled card "Apa yang baru" / "What's new"
   under the version row: the entries from `whats_new::latest(3)`, each a version
   label and its bullets, in the current UI language. No scroll of its own;
   the page already scrolls.
7. **Tests**: `since` (same, one behind, several behind, ahead, bad strings);
   the start decision as a pure function (fresh, upgrade, same, downgrade);
   notice kind -> effect mapping in `state.rs`; i18n ID/EN present for the new
   keys; the card layout fits at the Settings minimum width in both languages
   and both fonts (existing Settings layout tests' style); demo scene
   `whats-new` for the notice and the card.
8. **Docs**: new memory note `whats-new`, index line, README line under updates,
   a note in `self-update` that the "Diperbarui" notice now carries the link.

## Left to the builder

- Exact i18n wording (short, plain, matches the existing tone).
- The guard checks the version in `Cargo.toml` (5.3.1 today), so the table
  starts with a 5.3.1 entry written from `git log` of that release. Older
  versions are not backfilled. The next release adds its own entry before the
  bump.
- How `open_card` reaches Tentang (today it targets Fitur): extend it minimally.
