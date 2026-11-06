//! The island's own clipboard history (Run 7 of the v3.1 plan): the pure
//! parts (`kinds`, `image`, `store`, `notice`, `persist`'s format) are tested with
//! fake snapshots; `win` touches the real clipboard and is only called by
//! the running island. See docs/memory/clipboard-history.md and
//! clipboard-privacy.md.

pub mod image;
pub mod kinds;
pub mod notice;
pub mod pack;
pub mod persist;
pub mod store;
pub mod win;

pub use kinds::ClipKind;
pub use store::{Clip, ClipData, History};
