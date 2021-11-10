//! Asynchronously send data to the Wayland clipboard and listen for changes.
//!
//! This crate interacts with the Wayland clipboard using the `data-control` protocol from
//! [wlroots](https://github.com/swaywm/wlr-protocols).
//! If you are writing a Wayland desktop application that spawns windows, using this crate is
//! most likely not the correct approach.

mod eventloop;
mod cancel;
mod clipboard_internal;
mod clipboard_nice;

pub use clipboard_nice::WaylandClipboard;
pub use clipboard_internal::{ClipboardRequest, ClipboardOffer, PLAINTEXT_MIME_TYPES};
