use std::{thread::{self, JoinHandle}};

use tokio::sync::{oneshot, watch};
use windows::{Win32::Foundation::*, Win32::UI::WindowsAndMessaging::*, runtime::Error};

mod clipboard_thread;

pub mod offer;
pub mod raw;
pub mod format;

pub use offer::ClipboardOffer;

struct WindowsEventLoop {
    handle: HWND,
    thread: Option<JoinHandle<()>>,
}
impl Drop for WindowsEventLoop {
    fn drop(&mut self) {
        // send a WM_DESTROY and wait for the thread to end.
        // not ideal (it's blocking) but what can we do
        assert!(unsafe { PostMessageA(self.handle, WM_DESTROY, None, None) }.as_bool());
        // (what we could do is to use a oneshot that signals thread competion and offer a
        // consuming async destructor, but most likely not worth the effort)
        self.thread.take().unwrap().join().unwrap();
        // TODO: handle panics more gracefully
    }
}

/// Represents a thread running this crate's clipboard functionality.
///
/// - when not holding the clipboard: watch for clipboard updates
/// - when holding the clipboard: respond to requests for data
pub struct WindowsClipboard {
    _eventloop: WindowsEventLoop,
    pub rx_offers: watch::Receiver<Option<offer::ClipboardOffer>>,
}
impl WindowsClipboard {
    pub async fn init() -> Result<WindowsClipboard, Error> {
        let (tx, rx) = oneshot::channel();
        let (tx_offers, rx_offers) = watch::channel(read_clipboard(HWND(0))?);
        let thread = thread::spawn(move || clipboard_thread::run(tx_offers, tx));

        let handle = rx.await.unwrap();

        Ok(WindowsClipboard {
            _eventloop: WindowsEventLoop {
                handle,
                thread: Some(thread),
            },
            rx_offers
        })
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
