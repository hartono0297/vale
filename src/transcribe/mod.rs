//! Transcripts of recordings, offline, with transcribe.cpp.
//!
//! The engine is not compiled into `vale.exe`: the pinned native package
//! (`catalog::ENGINE`) is downloaded with a model when the user turns the
//! feature on, checked against its SHA-256, unpacked under
//! `%APPDATA%\Vale\transcribe\` (`store`) and loaded at run time over its C
//! API (`engine`). Nothing here runs while no download or transcription
//! does: no thread, no timer.
//!
//! The pipeline (`runner`): the `.m4a` (or `.wav`) is decoded per channel to
//! 16 kHz mono (`decode`), each channel is cut into speech chunks with their
//! start times (`chunk`), every chunk goes through the model, and the
//! segments of both channels are merged in time order, the mic's echo of the
//! meeting is dropped (`echo`) and the rest is written as `.md`
//! and `.srt` next to the audio (`output`).

// Until the Settings card and the transcript window are built (after track A
// is merged), parts of this module are reached only by the tests and the
// debug `--transcribe` flag.
#![allow(dead_code)]

pub mod catalog;
pub mod store;
pub mod download;
pub mod decode;
pub mod chunk;
pub mod output;
pub mod echo;
pub mod engine;
pub mod gpu;
pub mod runner;
pub mod queue;
pub mod doc;
pub mod prefs;
#[cfg(debug_assertions)]
pub mod debug;
