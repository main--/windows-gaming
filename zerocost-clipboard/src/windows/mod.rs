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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::watch;
use windows::Win32::System::DataExchange::{AddClipboardFormatListener, GetClipboardOwner, RemoveClipboardFormatListener};
use windows::Win32::System::SystemServices::CLIPBOARD_FORMATS;
use windows::{Win32::Foundation::*, Win32::UI::WindowsAndMessaging::*};

pub mod offer;
pub mod send;
pub mod raw;
pub mod format;

pub use offer::ClipboardOffer;
pub use send::{ClipboardContents, ClipboardFormatContent, ClipboardFormatData, DelayRenderedClipboardData};

/// Result type from the `windows` crate.
pub use windows::runtime::Result as WinapiResult;
use windows_eventloop::{RemoveMeError, WindowMessageListener, WindowsEventLoop};

use self::raw::WindowsClipboardOwned;

/// Represents this crate's clipboard functionality, running on a `WindowsEventLoop`.
/// It has the following responsibilities:
///
/// - when not holding the clipboard: watch for clipboard updates
/// - when holding the clipboard: respond to requests for data
///
/// # Shutdown
///
/// If you drop this struct (and all `watch::Receiver` instances obtained through `watch()`), we will
/// stop listening to clipboard updates as soon as all delay-rendered content has been taken care of.
///
/// Alternatively, you can simply shut down the underlying `WindowsEventLoop`.
/// This causes Windows to query for all delay-rendered contents immediately.
/// Hence, you should make sure that your delay-renderers remain working and responsive until `WindowsEventLoop::shutdown` has returned.
pub struct WindowsClipboard {
    hwnd: HWND,
    rx_offers: watch::Receiver<Option<offer::ClipboardOffer>>,
    delay_renderers: Arc<Mutex<HashMap<u32, DelayRenderedClipboardData>>>,
}

struct ClipboardHandler {
    upd: watch::Sender<Option<ClipboardOffer>>,
    delay_renderers: Arc<Mutex<HashMap<u32, DelayRenderedClipboardData>>>,
}
#[async_trait(?Send)]
impl WindowMessageListener for ClipboardHandler {
    async fn handle(&mut self, window: HWND, message: u32, wparam: WPARAM, _lparam: LPARAM) -> Result<Option<LRESULT>, RemoveMeError> {
        unsafe {
            match message {
                WM_CLIPBOARDUPDATE => {
                    let offer = super::read_clipboard(window).unwrap_or(None);
                    match self.upd.send(offer) {
                        // if there is nobody to receive, the WindowsClipboard has been dropped
                        // let's unregister if (and only if) we have no outstanding delay renderers
                        Err(_) if self.delay_renderers.lock().unwrap().is_empty() => {
                            RemoveClipboardFormatListener(window);
                            return Err(RemoveMeError);
                        }
                        _ => (),
                    }
                }
                WM_DESTROYCLIPBOARD => {
                    // if someone else takes over the clipboard, destroy our delay renderers
                    if GetClipboardOwner() != window {
                        self.delay_renderers.lock().unwrap().clear();
                    }
                }
                WM_RENDERFORMAT => {
                    let mut delay_renderers = self.delay_renderers.lock().unwrap();
                    let fmt = CLIPBOARD_FORMATS(wparam.0 as u32);
                    if let Some(dr) = delay_renderers.remove(&fmt.0) {
                        if let Some(cfd) = dr.delay_render().await {
                            let _ = cfd.render(&mut WindowsClipboardOwned::assert());
                            // if windows refuses to render this there is nothing we can do
                        }
                        // else: if our delay renderer us not responding there is nothing we can do
                    }
                    // else: if our delay renderer is already gone there is - you guessed it - nothing we can do

                    // TODO: maybe add debug logging for all of these cases
                }
                WM_RENDERALLFORMATS => {
                    let mut delay_renderers = self.delay_renderers.lock().unwrap();

                    let clipboard_open = raw::WindowsClipboard::open(window);
                    let mut clipboard = WindowsClipboardOwned::assert(); // must not clear existing data here
                    if GetClipboardOwner() == window { // someone else could have the clipboard by now
                        let joined = futures_util::future::join_all(delay_renderers.drain().map(|(_, renderer)| async move {
                            renderer.delay_render().await
                        })).await;

                        for cfd in joined.into_iter().flatten() {
                            let _ = cfd.render(&mut clipboard);
                            // if windows refuses to render this there is nothing we can do
                        }
                    }
                    drop(clipboard_open);
                }
                _ => return Ok(None),
            }

            Ok(Some(LRESULT(0)))
        }
    }
}

impl WindowsClipboard {
    /// Initialize this crate's clipboard functionality.
    ///
    /// # Concurrent instances
    ///
    /// Initializing a second `WindowsClipboard` instance for the same `WindowsEventLoop`
    /// while the first instance is still active is not allowed and causes an error.
    pub async fn init(wel: &WindowsEventLoop) -> windows::runtime::Result<WindowsClipboard> {
        let (tx_offers, rx_offers) = watch::channel(read_clipboard(HWND(0))?);

        let hwnd = wel.window_handle();
        let delay_renderers: Arc<Mutex<HashMap<u32, DelayRenderedClipboardData>>> = Default::default();
        let dr2 = delay_renderers.clone();
        wel.send_callback(Box::new(move |wd| {
            unsafe {
                assert!(AddClipboardFormatListener(hwnd).as_bool());
            }
            wd.register_listener(Box::new(ClipboardHandler { upd: tx_offers, delay_renderers }))
        })).await.unwrap();

        Ok(WindowsClipboard { hwnd, rx_offers, delay_renderers: dr2 })
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
    pub fn send(&self, contents: ClipboardContents) -> WinapiResult<()> {
        let mut clipboard = raw::WindowsClipboard::open(self.hwnd);
        let mut clipboard = clipboard.clear()?;
        let mut delay_renderers = self.delay_renderers.lock().unwrap();
        delay_renderers.clear();
        for content in contents.0 {
            match content {
                ClipboardFormatContent::DelayRendered(renderer) => {
                    let format = renderer.format();
                    delay_renderers.insert(format.0, renderer);
                    clipboard.send_delay_rendered(format)?;
                }
                ClipboardFormatContent::Immediate(val) => {
                    val.render(&mut clipboard)?;
                }
            }
        }
        Ok(())
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
