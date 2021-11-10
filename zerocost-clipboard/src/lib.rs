//! Asynchronously send data to the clipboard and listen for changes.
//!
//! # What is the point of this?
//!
//! The goal of this crate is to provide a zero-cost (or at least as zero-cost as possible) cross-platform clipboard abstraction.
//!
//! Cipboards across all platforms implement a pattern where applications can offer data on the clipboard while
//! only following up with the contents if they actually end up getting pasted somewhere.
//!
//! Rust already has several other good clipboard crates (`clipboard`, `wl-clipboard-rs`, etc).
//! Sadly, they offer neither the ability to efficiently listen for changes nor the ability to paste
//! efficiently using the above pattern.
//!
//! ### Supported Clipboard implementations
//!
//! - Wayland
//! - Windows
//!
//! TODO: X11

cfg_if::cfg_if! {
    if #[cfg(all(unix, feature = "wayland"))] {
        pub mod wayland;
        pub use crate::wayland::*;
    } else if #[cfg(all(windows, feature = "win-clipboard"))] {
        pub mod windows;
        pub use crate::windows::*;
    }
}
