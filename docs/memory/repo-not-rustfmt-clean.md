---
tags: [convention, rust, tooling]
date: 2026-11-14
---

# Do not run cargo fmt on the repo

The repo is not rustfmt-clean (it was formatted once, see [[v3-3-handoff]],
and has drifted since). `cargo fmt` reformats about 156 files and buries a
real change in noise (it happened on 2026-11-14 and the edits had to be
redone). Format new code by hand to match the file around it; rustfmt only
lines you wrote, if at all.
