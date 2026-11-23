---
tags: [gotcha, tests, disk]
date: 2026-11-14
---

# Tests leave hundreds of MB in target/test-tmp

A full `cargo test --locked` leaves `target/test-tmp/` behind: about 513 MB of
`transcribe-sweep-models-<pid>` per run, plus `rec-*` folders. Repeated runs
filled the D: drive during the mascot work (2026-11-13/14). Run
`rm -rf target/test-tmp` after a full run until the tests clean up after
themselves (not fixed yet). On 2026-11-23 the main checkout's
`target/test-tmp` had grown to 18 GB (751 folders) while three sessions built
in parallel, D: hit 0 bytes free and builds failed with os error 112; deleting
only the folders whose `<pid>` is no longer running freed it. The `app::record` test probing `Z:\no\such\drive`
fails on a PC that has a Z: drive; that failure is the environment, not code.
