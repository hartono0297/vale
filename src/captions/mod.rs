//! Live captions: speech on the PC (and the mic) as text while it is spoken.

// Until the caption worker drives it.
#[cfg_attr(not(test), allow(dead_code))]
pub mod live;

// Until the caption worker drives it.
#[cfg_attr(not(test), allow(dead_code))]
pub mod lines;

// Until the caption worker drives it.
#[cfg_attr(not(test), allow(dead_code))]
pub mod control;

// Until the caption worker drives it.
#[cfg_attr(not(test), allow(dead_code))]
pub mod prefs;

// Until the island drives it (the debug CLI does, in debug builds).
#[cfg_attr(not(test), allow(dead_code))]
pub mod worker;

#[cfg(debug_assertions)]
pub mod debug;
