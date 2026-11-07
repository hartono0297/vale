//! Screen recording (track C core): Windows Graphics Capture, a D3D11
//! compositor for the zoom and the cursor, and a Media Foundation sink
//! writer to MP4. Nothing here runs, and no thread, capture or timer exists,
//! until a recording starts.

// Not wired into the app until track A lands; the debug CLI
// (`--screenrec-test`) is the only caller in a debug build.
#![cfg_attr(not(debug_assertions), allow(dead_code))]

pub mod cursor_log;
pub mod files;
pub mod plan;
pub mod zoom;
