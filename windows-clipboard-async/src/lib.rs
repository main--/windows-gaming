//! Asynchronously listen to the Windows clipboard and send delay-rendered data to it.
//!
//! # Example
//!
//! ```
//! use windows_clipboard_async::*;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() {
//!     let clipboard = WindowsClipboard::init().await.unwrap();
//!
//!     let mut watch = clipboard.watch();
//!     let watcher = tokio::spawn(async move {
//!         watch.changed().await.unwrap();
//!         watch.borrow_and_update().as_ref().unwrap().receive_string().unwrap()
//!     });
//!
//!     clipboard.send(ClipboardContents(vec![
//!         (format::CF_UNICODETEXT,
//!             ClipboardFormatContent::Immediate(ClipboardFormatData::Text("stonks".to_owned())))
//!     ])).await.unwrap();
//!
//!     assert_eq!(watcher.await.unwrap(), "stonks");
//! }
//! ```

use std::{any::Any, panic::AssertUnwindSafe, thread};

use tokio::{runtime::Handle, sync::{mpsc, oneshot, watch}};
use windows::{Win32::Foundation::*, Win32::UI::WindowsAndMessaging::*};

mod clipboard_thread;

pub mod offer;
pub mod send;
pub mod raw;
pub mod format;

pub use offer::ClipboardOffer;
pub use send::{ClipboardContents, ClipboardFormatContent, ClipboardFormatData};

/// Result type from the `windows` crate.
pub use windows::runtime::Result as WinapiResult;

struct WindowsEventLoop {
    handle: HWND,
    rx_shutdown: Option<oneshot::Receiver<Result<WinapiResult<()>, Box<dyn Any + Send>>>>,
}
impl WindowsEventLoop {
    ///
    async fn shutdown(mut self) -> WinapiResult<()> {
        let rx_shutdown = self.rx_shutdown.take().unwrap();
        drop(self);
        match rx_shutdown.await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => std::panic::resume_unwind(e),
            Err(_) => unreachable!(),
        }
    }
}
impl Drop for WindowsEventLoop {
    fn drop(&mut self) {
        unsafe { PostMessageA(self.handle, WM_USER, None, None) }.ok().unwrap();
    }
}

/// Represents a thread running this crate's clipboard functionality.
///
/// - when not holding the clipboard: watch for clipboard updates
/// - when holding the clipboard: respond to requests for data
pub struct WindowsClipboard {
    eventloop: WindowsEventLoop,
    rx_offers: watch::Receiver<Option<offer::ClipboardOffer>>,
    tx_contents: mpsc::Sender<ClipboardContents>,
}


impl WindowsClipboard {
    /// Initialize this crate's clipboard functionality.
    pub async fn init() -> windows::runtime::Result<WindowsClipboard> {
        let (tx, rx) = oneshot::channel();
        let (tx_offers, rx_offers) = watch::channel(read_clipboard(HWND(0))?);
        let (tx_contents, rx_contents) = mpsc::channel(1);
        let handle = Handle::current();

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        thread::spawn(move || {
            let res = std::panic::catch_unwind(AssertUnwindSafe(move || clipboard_thread::run(handle, tx_offers, rx_contents, tx)));
            match tx_shutdown.send(res) {
                Ok(()) => (), // they received it
                Err(Ok(Ok(()))) => (), // no receiver it but it's no crash
                Err(Ok(Err(_))) => {
                    // no receiver but we failed.
                    // it's not a panic either however, so can't be that serious i guess?
                    // maybe let's not kill the program in this case
                }
                Err(Err(e)) => {
                    // no receiver and we had a crash
                    // no choice but to rethrow and bring down the application
                    std::panic::resume_unwind(e);
                }
            }
        });


        let handle = match rx.await {
            Ok(h) => h,
            Err(_) => {
                // the only way this can happen is if they crashed; let's propagate the error or panic
                match rx_shutdown.await.unwrap() {
                    Ok(Ok(())) => unreachable!(),
                    Ok(Err(e)) => return Err(e),
                    Err(e) => std::panic::resume_unwind(e),
                }
            }
        };

        let eventloop = WindowsEventLoop { handle, rx_shutdown: Some(rx_shutdown) };
        Ok(WindowsClipboard {
            eventloop,
            rx_offers,
            tx_contents,
        })
    }


    /// Watch for clipboard changes.
    ///
    /// Whenver the clipboard is updated by anyone (including you, the user of this library),
    /// a new [`ClipboardOffer`] is received by this channel.
    pub fn watch(&self) -> watch::Receiver<Option<offer::ClipboardOffer>> {
        self.rx_offers.clone()
    }

    /// Get the current clipboard contents.
    pub fn current(&self) -> Option<offer::ClipboardOffer> {
        self.rx_offers.borrow().clone()
    }

    /// Send new contents to the clipboard.
    ///
    /// Note that this sends through a channel internally, so the fact that this future returns does not mean that
    /// your data is already visible on the clipboard.
    /// You would have to wait and check with `watch` for that.
    pub async fn send(&self, contents: ClipboardContents) -> Result<(), mpsc::error::SendError<ClipboardContents>> {
        self.tx_contents.send(contents).await
    }

    /// Shut down the clipboard thread.
    ///
    /// This causes Windows to query for all delay-rendered contents immediately.
    /// Hence, you should make sure that your delay-renderers remain working and responsive until this function has returned.
    ///
    /// For completeness' sake, note that this call will re-surface any errors or panics that happened on the clipboard thread.
    /// However, the clipboard thread is not expected to panic in general.
    pub async fn shutdown(self) -> WinapiResult<()> {
        self.eventloop.shutdown().await
    }
}

/// Open the clipboard and check for available offers.
pub fn read_clipboard(window: HWND) -> windows::runtime::Result<Option<ClipboardOffer>> {
    let clipboard = raw::WindowsClipboard::open(window);
    let formats: Vec<_> = clipboard.enum_formats().collect::<windows::runtime::Result<_>>()?;
    let offer = if formats.is_empty() {
        None
    } else {
        let sequence = clipboard.sequence_number();
        Some(ClipboardOffer::new(sequence, formats))
    };
    Ok(offer)
}
