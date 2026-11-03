//! Desktop widgets (phase 1: sticky notes). The pure rules live in `prefs`,
//! `sticky`, `checklist`, `snap` and `place` and are tested without a window;
//! `host` owns the layered windows, `edit` their keyboard, `paint` their
//! drawing, and `demo` is the debug scene.

pub mod apply;
pub mod checklist;
pub mod place;
pub mod prefs;
pub mod snap;
pub mod sticky;

pub mod edit;
pub mod host;
pub mod paint;
pub mod pop;

#[cfg(debug_assertions)]
pub mod demo;

use std::sync::atomic::{AtomicUsize, Ordering};

/// How many sticky notes are on the desktop now, kept for the tray menu,
/// which is built from the config alone. Set by the island on every sync.
static STICKY_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn set_sticky_count(n: usize) {
    STICKY_COUNT.store(n, Ordering::Relaxed);
}

pub fn sticky_count() -> usize {
    STICKY_COUNT.load(Ordering::Relaxed)
}
