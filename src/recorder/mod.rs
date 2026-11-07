//! Audio recording: the mic, the system audio (WASAPI loopback) or both to an
//! `.m4a` with a `.vale.json` sidecar. Design:
//! `docs/superpowers/specs/2026-11-07-recording-design.md`; plan:
//! `docs/superpowers/plans/2026-11-07-recording-phase1.md`.

pub mod encode;
pub mod level;
pub mod naming;
pub mod resample;
pub mod sidecar;
pub mod timeline;
