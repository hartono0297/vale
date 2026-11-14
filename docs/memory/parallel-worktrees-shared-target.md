---
tags: [convention, tooling, disk, agents]
date: 2026-11-15
---

# Parallel story agents: one shared target dir, focused tests

Part 2 of [[mascot-play]] ran six stories at once, each in its own worktree
(`.claude/worktrees/p2-tNN`, branch per story), all sharing one
`CARGO_TARGET_DIR`. A target dir is ~5 GB, so one each would have filled D:.
Cargo's build lock then serialises every build and test run: six agents each
running the full suite queued for hours. What worked: agents run only the
focused filter (`cargo test --locked ui::play` + `cargo build`), and the full
suite runs once after merging. Expect merge conflicts only in the shared
registries (`stories/mod.rs` `ready`/`shot`, `demo.rs` `SCENES` count, the
pool test); derive tests from the registry instead of listing stories.
`target/test-tmp` lives under each worktree's own `target/`
([[test-tmp-leak]]), not the shared dir.
