//! Desktop widgets (phase 1: sticky notes). The pure rules live in `prefs`,
//! `sticky`, `checklist`, `snap`, `place`, `stack` and `tuck` and are tested without a window;
//! `host` owns the layered windows, `edit` their keyboard, `paint` their
//! drawing, and `demo` is the debug scene.

pub mod apply;
pub mod art;
pub mod checklist;
pub mod cliptext;
pub mod images;
pub mod list;
pub mod place;
pub mod prefs;
pub mod remind;
pub mod richclip;
pub mod snap;
pub mod stack;
pub mod sticky;
pub mod styles;
pub mod templates;
pub mod toolbar;
pub mod tuck;

pub mod clip_io;
pub mod drop;
pub mod edit;
pub mod fmt_pop;
pub mod host;
pub mod image_io;
pub mod list_win;
pub mod mood;
pub mod paint;
pub mod pop;
pub mod remind_pop;

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
