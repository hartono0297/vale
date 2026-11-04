# Agent rules

## Models per phase (set in `.claude/workflows/build.js`)

| Phase | Model | Effort |
|---|---|---|
| Plan | Opus 5.5 (`claude-opus-5-5`) | high |
| Build (write the code) | Sonnet 5.5 (`claude-sonnet-5-5`) | high |
| Review | Opus 5.5 (`claude-opus-5-5`) | high |
| Fix | Sonnet 5.5 (`claude-sonnet-5-5`) in round 1, Opus 5.5 (`claude-opus-5-5`) from round 2 if the reviewer still says FIX; Opus for a failed check | high |
| Check | Sonnet 5.5 (`claude-sonnet-5-5`) | high |
| Ship (merge, test) | Sonnet 5.5 (`claude-sonnet-5-5`); a red `main` goes to an Opus fixer | medium (fixer high) |
| Finish (memory, README, push) | Sonnet 5.5 (`claude-sonnet-5-5`) | medium |
| Clean (worktrees, branches) | Sonnet 5.5 (`claude-sonnet-5-5`) | medium |

- Spawning subagents (Agent tool) is allowed whenever it helps: parallel
  reading, independent tasks, blind reviewers, fixers. Set each agent's model
  from the table above for the phase it works in.
- Review and fix loop until the review comes back clean (up to 3 rounds).
- If a model cannot be selected (for example `model_not_found`), say so and
  ask before falling back to another model.

## Every code change goes through the `build` workflow

The workflow is the one from Gxl-WorkFlow (`D:\GitHub\Gxl-WorkFlow`,
`template/.claude/workflows/build.js`), copied unchanged with its model rules
(the table above). To update it, copy the template file again.

I opt in to multi-agent orchestration for this project. Whenever I ask for a code
change (new feature, bug fix, refactor), run the `build` workflow
(`.claude/workflows/build.js`) with my request as `args`. Do not edit the code
yourself in the main session.

- Do not weigh the tasks yourself. The workflow's Plan step (Opus) splits the
  request and marks each task `light` or `heavy`. Light tasks get build only
  (the builder and Ship run the tests). Heavy tasks get build, a blind review of
  a pinned commit (fixes land in the builder's worktree), and a check against a
  written checklist. A failed check item goes back to a fix (Opus), then one more
  review; only a SHIP is checked again, and a task gets 2 checks at most. What
  the checker cannot test lands in `manualChecks` for me.
- The workflow merges passing tasks into `main` locally and runs the tests. If
  `main` is red, an Opus fixer commits a fix on `main`, it gets a blind review,
  and the tests run again (2 fix rounds at most). Once everything is
  ready (every task merged, reviewed, fixed and checked where required, tests
  green on `main`, nothing `incomplete`), its Finish step **always** updates the
  Obsidian memory and `README.md`, commits those docs, and pushes `main`. This
  project uses a single branch (`main`), so that push is pre-approved. Deploy
  stays with me.
- Once that push succeeded, its Clean step removes the worktrees and branches
  the run left behind (never forced). Anything not merged into `main` or with
  changes is kept and reported. If Finish did not run or the push failed,
  nothing is removed and the result lists them in `leftovers`.
- If anything is not ready, the workflow skips docs and push. Fix what it
  reports first; don't push half-finished work from the main session.
- For a long batch of runs, pass `args: {request, defer: true}` to build now,
  then run one verify pass with `args: {verify: [...]}` (the `deferred` list the
  runs returned) at the end. Deferred runs never push; the verify pass does the
  Finish step.
- When it finishes, report: which tasks were merged and their weight, what the
  Finish step wrote to memory and README, whether `main` was pushed, what Clean
  removed (and the space freed) or kept, `manualChecks`, `incomplete`, and any
  open findings or failed checks.

## Clean up worktrees once they are pushed (main session)

Clean only touches what its own run created. The main session cleans the rest
the same way, once its branch is merged into `main` and `main` is pushed:
worktrees you create yourself with the Agent tool's `isolation: "worktree"`, and
a run's `leftovers` once what held them is fixed and pushed.

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
a new task that would run `build` (new feature, bug fix, refactor), do not
brainstorm or plan it here. Start a new session for it right away, without
asking me first, and tell me in one line which session it is.

- Start it with `start_session`, context `fresh`. If this session has no
  `start_session` tool, use `spawn_task` instead (the Claude desktop app shows it
  as a card; one click starts the session). The brief holds my request in my own
  words plus whatever from this conversation it needs, stands on its own, and
  says: "You were started by the main session to brainstorm and plan this task
  with the user, then run `build`. Follow 'Always brainstorm before `build`' in
  AGENTS.md. Skip 'Session start'. Do not start another session."
- A session started that way brainstorms and plans in itself and runs `build`
  itself. This rule does not apply inside it, so it never spawns another one:
  anything I add or change there, even a new request, stays in that session.
- One session per task. If I give several tasks at once, start one session each.
- Follow-ups, corrections or answers about a task that already has a session go
  to that session (`SendMessage`), not to a new one.
- What is not a new task stays here: questions, explanations, reading code, and
  anything under "When NOT to run it".
- Do not brainstorm, read code for the task, or run `build` in the main
  session while waiting for the new one. Only if neither `start_session` nor
  `spawn_task` exists, say so and brainstorm in the current session.

## Always brainstorm before `build`

Agents inside the workflow cannot ask me anything, so every open question gets
settled first, in the session that owns the task, with me. Before every `build`
run:

- Restate what you understood in a few lines, then ask about whatever is
  unclear: the goal, who uses it, the exact behaviour and edge cases, what must
  not change, and what "done" means. Ask a few focused questions at a time.
- For larger or open-ended work, propose 2-3 approaches with trade-offs and
  recommend one.
- Even for a small, clear request, restate it in one or two lines and wait for
  my OK.
- Only when I confirm, run `build` with the agreed request written out
  concretely, including the acceptance criteria.

## Session start (main session only)

Runs only in the session I first talk to in this project, never in a session
started from it (those skip it; the main session already did it). No commands
unless a check says so:

- If `.claude/preflight.done` does not exist, read `.claude/setup/session-start.md`
  and follow its "Preflight" section.
- If a `superpowers:*` skill is in your skill list, read the same file and follow
  its "Superpowers" section. With none in the list, do nothing.

## Tools (main session and every workflow agent)

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
    index. The `build` workflow does this in its Finish step.
  - Don't save what the code or git history already records. Update an
    existing note instead of creating a duplicate.

## When NOT to run it

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
2. **Release** (only when I ask for a release): bump the version in
   `Cargo.toml`, `Cargo.lock` (`cargo update --workspace --offline`),
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

If the tests fail, or another session still has a `build` running, stop and
tell me instead of pushing.
