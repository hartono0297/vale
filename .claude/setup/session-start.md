# Session start: Preflight

Read only when `AGENTS.md` ("Session start") sends you here. Every agent loads
`AGENTS.md`, so the long rules live in this file instead.

Any shell check on settings files ends with `|| true`, so a missing file does not
turn the call into a failure.

## Preflight: once per project

Runs when `.claude/preflight.done` does not exist.

Before the first task, check that the three tools below are in place and
install whatever is missing. Then create `.claude/preflight.done` (today's date
+ what was installed), commit it, and report what you installed.

| Tool | Check | If missing, install |
|---|---|---|
| RTK | `rtk --version`, and `~/.claude/settings.json` has a Bash `PreToolUse` hook running `rtk hook claude` | Windows: `winget install --id rtk-ai.rtk -e`. Then `rtk init -g --auto-patch` |
| Context7 | `claude mcp list` shows `context7` | `claude mcp add --scope user --transport http context7 https://mcp.context7.com/mcp` |
| Obsidian vault | `docs/memory/index.md` exists in this project | Create it with the Decisions / Gotchas / Conventions sections and commit it. If the Obsidian app is missing (Windows: `winget list --id Obsidian.Obsidian`), install it with `winget install --id Obsidian.Obsidian -e` |

- A newly added MCP server only loads in a new session. If Context7 was just
  installed, tell me to restart Claude Code before starting work.
- Opening `docs/` as a vault in the Obsidian app is my step; remind me if it's new.
- To run it again (e.g. on a new machine), I delete `.claude/preflight.done`.
