//! Live captions: speech on the PC (and the mic) as text while it is spoken.

pub mod live;

pub mod lines;

pub mod control;

pub mod prefs;

pub mod worker;

#[cfg(debug_assertions)]
pub mod debug;
