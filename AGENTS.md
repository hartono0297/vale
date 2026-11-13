# Agent rules

## Every code change goes through Superpowers

Superpowers (`superpowers@claude-plugins-official`) is on for this project
(`.claude/settings.json`). Whenever I ask for a code change (new feature, bug
fix, refactor), follow its flow:

1. `superpowers:brainstorming`: settle the goal, the exact behaviour, edge cases,
   what must not change and what "done" means, with me, and wait for my OK.
2. `superpowers:writing-plans`: write the plan.
3. `superpowers:subagent-driven-development` (or `executing-plans`): implement it
   test-first (`test-driven-development`), with reviews
   (`requesting-code-review`). Use `systematic-debugging` for bugs and
   `verification-before-completion` before claiming anything works.
4. `superpowers:finishing-a-development-branch`: this project uses a single
   branch, so always pick **merge locally into `main`**. Then run
   `cargo test --locked` on `main`.
5. Once `main` is green and everything is merged: update the Obsidian memory and
   `README.md` (see "Tools"), commit them as "docs: update memory and README",
   and push `main`. That push is pre-approved. Deploy stays with me.
6. After the push, clean up the worktrees and branches the change left behind
   (see "Clean up worktrees").

- Put worktrees under `.claude/worktrees/` (gitignored).
- If anything is not ready (tests red, review findings open, a task unfinished),
  skip docs and push and tell me what is left.
- When it finishes, report what was merged, what was written to memory and
  README, whether `main` was pushed, what cleanup removed (and the space freed)
  or kept, what I still have to check by hand, and any open findings.
- The old `build` workflow (`.claude/workflows/build.js`) stays in the repo but
  is not used.

## Models

Let Superpowers pick each subagent's model with its own "Model Selection" rules
(`subagent-driven-development`): the cheapest model for mechanical work, a
standard one for integration and debugging, the most capable one for design and
the final whole-branch review. Always name the model explicitly when spawning a
subagent. If a model cannot be selected (for example `model_not_found`), say so
and ask before falling back to another model.

## Clean up worktrees once they are pushed

Once a branch is merged into `main` and `main` is pushed, remove its worktree
and branch:

- If a folder in the worktree is a link to a folder outside it (a Windows
  junction or a symlink, such as a shared `node_modules`), remove only the link
  first (`cmd /c rmdir "<link>"` on Windows, `rm "<link>"` elsewhere), never
  recursively: `git worktree remove` follows a junction and deletes its target.
- `git worktree remove <path>`, never `--force`. If only untracked build output
  (such as `target/`) blocks it, delete that folder first.
- `git branch -d` its branch and the tool's own `worktree-<folder>` branch,
  never `-D`. Then `git worktree prune`.
- Skip anything not merged into `main`, with changes, or still used by a running
  session, and report it (path, branch, why). Report what was removed and
  roughly how much space it freed.

## Every new task starts in a new session

The main session only takes tasks in; the thinking happens elsewhere. When I give
a new code-change task (new feature, bug fix, refactor), do not brainstorm or
plan it here. Start a new session for it right away, without asking me first,
and tell me in one line which session it is.

- Start it with `start_session`, context `fresh`. If this session has no
  `start_session` tool, use `spawn_task` instead (the Claude desktop app shows it
  as a card; one click starts the session). The brief holds my request in my own
  words plus whatever from this conversation it needs, stands on its own, and
  says: "You were started by the main session to brainstorm and plan this task
  with the user, then implement it. Follow 'Every code change goes through
  Superpowers' in AGENTS.md. Skip 'Session start'. Do not start another session."
- A session started that way brainstorms, plans and implements in itself. This
  rule does not apply inside it, so it never spawns another one: anything I add
  or change there, even a new request, stays in that session.
- One session per task. If I give several tasks at once, start one session each.
- Follow-ups, corrections or answers about a task that already has a session go
  to that session (`SendMessage`), not to a new one.
- What is not a new task stays here: questions, explanations, reading code, and
  anything under "When not to use the flow".
- Do not brainstorm, read code for the task, or implement it in the main session
  while waiting for the new one. Only if neither `start_session` nor `spawn_task`
  exists, say so and brainstorm in the current session.

## Session start (main session only)

Runs only in the session I first talk to in this project, never in a session
started from it. If `.claude/preflight.done` does not exist, read
`.claude/setup/session-start.md` and follow its "Preflight" section. Otherwise
run no command.

## Tools (main session and every subagent)

- **RTK**: shell output goes through RTK to save tokens. The global Bash hook
  rewrites commands. Prefer `rtk git`, `rtk test`, `rtk grep`, etc., and use
  `rtk bypass <cmd>` only when a result is unusable.
- **Context7**: for any library, framework, SDK, CLI or API, look up current
  docs with the Context7 MCP (`resolve-library-id`, then `query-docs`) before
  answering or writing code. Match the version the project uses.
- **Obsidian memory**: project memory lives in the Obsidian vault at `docs/`.
  Notes go in `docs/memory/`, and the index is `docs/memory/index.md`.
  - The session that owns a task reads the index and the notes relevant to it
    before starting. A session that only hands the task off does not, and nobody
    lists the notes folder.
  - After work that taught something worth keeping (a decision, gotcha,
    convention), add or update one note per fact. Give it frontmatter with
    `tags` and `date`, `[[wikilinks]]` to related notes, and a link from the
    index. Step 5 of the Superpowers flow does this before the push.
  - Don't save what the code or git history already records. Update an
    existing note instead of creating a duplicate.

## When not to use the flow

- Questions, explanations, reading code, or planning without changes.
- Edits that are not code: docs, README, comments, config values, typos.
- When I explicitly say "do it directly" / "tanpa workflow".

## Project commands

Rust (edition 2024), Windows 11 x64 only. Needs the stable Rust toolchain with
the MSVC target (`rustup`, `cargo`); CI builds with `cargo build --release --locked`.

- Install: `rustup toolchain install stable --profile minimal` (deps come via cargo)
- Build: `cargo build --release --locked` → `target/release/vale.exe`
- Test: `cargo test --locked` (unit tests live inside `src/**` as `#[cfg(test)]` modules)
- Run locally: `cargo run --release`. The island is a single-instance UI at the
  top of the screen; stop any installed copy first (`Stop-Process -Name vale, dynamic-island`).
- Demo and debug modes for visuals (no real user data, no media keys):
  `--demo`, `--demo-editor`, `--demo-cli`, `--demo-robot`, `--demo-devices`, `--settings`.
- Release: push a `v*` tag; `.github/workflows/release.yml` builds and publishes.

## Push and release (main session, this project only)

When I say "push" or "push dan rilis", do these in order:

1. **Docs first.** Before pushing, check what landed since the last tag
   (`git log <last tag>..HEAD`) that the Obsidian memory (`docs/memory/`, index
   included) or `README.md` does not describe yet, and update them. Commit them
   as "docs: update memory and README". If nothing is missing, say so.
2. **Release** (only when I ask for a release): first write the What's new
   entry for the new version in `src/whats_new.rs` (Indonesian and English,
   2-5 plain points for users; `cargo test --locked` fails without it), then
   bump the version in `Cargo.toml`, `Cargo.lock` (`cargo update --workspace --offline`),
   `app.manifest` and `packaging/AppxManifest.xml`; run `cargo test --locked`;
   commit "chore(release): version X.Y.Z"; push `main`; then
   `git tag -a vX.Y.Z -m "Vale X.Y.Z"` and push the tag. Wait for the release
   workflow and verify that the latest release has `vale.exe`,
   `vale.exe.sha256` and the zip, and that the checksum matches.
3. **Clean up after the push.** Remove worktrees and branches that are merged
   and pushed (the rules in "Clean up worktrees once they are pushed"), and
   delete the sessions this session started for this project whose work is
   merged and pushed and that are idle. Never touch sessions of other projects
   or sessions with running work; report what was removed and what was kept.

If the tests fail, or another session is still in the middle of a code change
(implementing or merging), stop and tell me instead of pushing.
