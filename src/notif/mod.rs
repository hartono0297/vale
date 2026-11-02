//! The notification history (the Notifikasi page): a capped list of the
//! Windows toasts the island already catches, optionally kept in an
//! encrypted file. The pure parts (`store`, `time`, `tile`, the `persist`
//! format) are tested without Windows; `app_notif` is the island's side. See
//! docs/memory/notification-history.md.

pub mod claude_open;
pub mod open;
pub mod persist;
pub mod store;
pub mod tile;
pub mod time;

pub use store::{NotifEntry, NotifHistory, logo_key, route_toast};
